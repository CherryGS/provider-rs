//! Stateless access to Codex Responses through the ChatGPT backend.
//!
//! The request fields and streaming behavior are materially adapted from
//! `openai/codex` files `codex-rs/codex-api/src/common.rs`,
//! `codex-rs/codex-api/src/endpoint/responses.rs`, and
//! `codex-rs/core/src/client.rs` at pinned commit
//! `50ef7395faee1d0e2d01730f9636aa06091c7be3`:
//! <https://github.com/openai/codex/tree/50ef7395faee1d0e2d01730f9636aa06091c7be3>.

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, error, fmt, mem, str};

pub use crate::Credentials;
use crate::ExposeSecret;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
const USER_AGENT: &str = concat!("provider-codex/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StreamOptions {
    pub reasoning_summary_delivery: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TextControls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TextFormat {
    pub r#type: String,
    pub strict: bool,
    pub schema: Value,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub model: String,
    pub instructions: String,
    pub input: Vec<Value>,
    pub tools: Option<Vec<Value>>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<Reasoning>,
    pub stream_options: Option<StreamOptions>,
    pub include: Vec<String>,
    pub service_tier: Option<String>,
    pub prompt_cache_key: Option<String>,
    pub text: Option<TextControls>,
    pub client_metadata: Option<BTreeMap<String, String>>,
}

impl Request {
    pub fn new(model: impl Into<String>, input: Vec<Value>) -> Self {
        Self {
            model: model.into(),
            instructions: String::new(),
            input,
            tools: None,
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            reasoning: None,
            stream_options: None,
            include: vec!["reasoning.encrypted_content".to_string()],
            service_tier: None,
            prompt_cache_key: None,
            text: None,
            client_metadata: None,
        }
    }
}

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    instructions: &'a str,
    input: &'a [Value],
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a [Value]>,
    tool_choice: &'a str,
    parallel_tool_calls: bool,
    reasoning: Option<&'a Reasoning>,
    store: bool,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<&'a StreamOptions>,
    include: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a TextControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_metadata: Option<&'a BTreeMap<String, String>>,
}

impl<'a> From<&'a Request> for WireRequest<'a> {
    fn from(request: &'a Request) -> Self {
        Self {
            model: &request.model,
            instructions: &request.instructions,
            input: &request.input,
            tools: request.tools.as_deref(),
            tool_choice: &request.tool_choice,
            parallel_tool_calls: request.parallel_tool_calls,
            reasoning: request.reasoning.as_ref(),
            store: false,
            stream: true,
            stream_options: request.stream_options.as_ref(),
            include: &request.include,
            service_tier: request.service_tier.as_deref(),
            prompt_cache_key: request.prompt_cache_key.as_deref(),
            text: request.text.as_ref(),
            client_metadata: request.client_metadata.as_ref(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug)]
pub struct EventStream {
    response: reqwest::Response,
    buffer: Vec<u8>,
    finished: bool,
    pub request_id: Option<Box<str>>,
    pub model: Option<Box<str>>,
    pub turn_state: Option<Box<str>>,
}

impl EventStream {
    pub async fn next(&mut self) -> Result<Option<Event>, Error> {
        loop {
            let frame = if let Some(frame) = take_frame(&mut self.buffer) {
                frame
            } else if self.finished {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                mem::take(&mut self.buffer)
            } else {
                match self.response.chunk().await.map_err(Error::Exchange)? {
                    Some(chunk) => {
                        self.buffer.extend_from_slice(&chunk);
                        continue;
                    }
                    None => {
                        self.finished = true;
                        continue;
                    }
                }
            };

            match parse_frame(&frame)? {
                ParsedFrame::Event(event) => return Ok(Some(event)),
                ParsedFrame::Done => {
                    self.finished = true;
                    self.buffer.clear();
                    return Ok(None);
                }
                ParsedFrame::Ignore => {}
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
    InvalidRequest(&'static str),
    Exchange(reqwest::Error),
    Response { status: StatusCode, body: Box<str> },
    Utf8(str::Utf8Error),
    Decode(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials(field) => write!(formatter, "Codex {field} is empty"),
            Self::InvalidRequest(field) => write!(formatter, "Codex Responses {field} is empty"),
            Self::Exchange(error) => write!(formatter, "Codex Responses request failed: {error}"),
            Self::Response { status, .. } => {
                write!(formatter, "Codex Responses request returned {status}")
            }
            Self::Utf8(error) => write!(formatter, "invalid Codex Responses event text: {error}"),
            Self::Decode(error) => write!(formatter, "invalid Codex Responses event: {error}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Exchange(error) => Some(error),
            Self::Utf8(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::InvalidCredentials(_) | Self::InvalidRequest(_) | Self::Response { .. } => None,
        }
    }
}

pub async fn call(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
) -> Result<EventStream, Error> {
    stream_from(client, credentials, request, ENDPOINT).await
}

async fn stream_from(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<EventStream, Error> {
    if credentials.access_token.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("access token"));
    }
    if credentials.account_id.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("account ID"));
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }

    let response = client
        .post(endpoint)
        .bearer_auth(credentials.access_token.expose_secret())
        .header("ChatGPT-Account-Id", credentials.account_id.expose_secret())
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .json(&WireRequest::from(request))
        .send()
        .await
        .map_err(Error::Exchange)?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(Error::Exchange)?
            .into_boxed_str();
        return Err(Error::Response { status, body });
    }

    let request_id = response_header(&response, "x-request-id");
    let model = response_header(&response, "openai-model")
        .or_else(|| response_header(&response, "x-openai-model"));
    let turn_state = response_header(&response, "x-codex-turn-state");

    Ok(EventStream {
        response,
        buffer: Vec::new(),
        finished: false,
        request_id,
        model,
        turn_state,
    })
}

fn response_header(response: &reqwest::Response, name: &str) -> Option<Box<str>> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(Into::into)
}

fn take_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let delimiter = [
        buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| (index, 4)),
        buffer
            .windows(2)
            .position(|window| window == b"\n\n")
            .map(|index| (index, 2)),
        buffer
            .windows(2)
            .position(|window| window == b"\r\r")
            .map(|index| (index, 2)),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(index, _)| *index)?;

    let (index, length) = delimiter;
    let mut frame: Vec<_> = buffer.drain(..index + length).collect();
    frame.truncate(index);
    Some(frame)
}

enum ParsedFrame {
    Event(Event),
    Done,
    Ignore,
}

fn parse_frame(frame: &[u8]) -> Result<ParsedFrame, Error> {
    let frame = str::from_utf8(frame).map_err(Error::Utf8)?;
    let mut data = String::new();
    let mut saw_data = false;

    for line in frame.lines() {
        let value = if line == "data" {
            ""
        } else if let Some(value) = line.strip_prefix("data:") {
            value.strip_prefix(' ').unwrap_or(value)
        } else {
            continue;
        };

        if saw_data {
            data.push('\n');
        }
        data.push_str(value);
        saw_data = true;
    }

    if !saw_data {
        return Ok(ParsedFrame::Ignore);
    }
    if data == "[DONE]" {
        return Ok(ParsedFrame::Done);
    }

    serde_json::from_str(&data)
        .map(ParsedFrame::Event)
        .map_err(Error::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
    };

    fn serve_once(status: &str, headers: &str, body: &str) -> (String, JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let status = status.to_owned();
        let headers = headers.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }

            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();

            request
        });
        (endpoint, handle)
    }

    #[tokio::test]
    async fn streams_response_events() {
        let body = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\"}}\n\n",
            "data: [DONE]\n\n",
        );
        let (endpoint, server) = serve_once(
            "200 OK",
            "X-Request-Id: req_123\r\nOpenAI-Model: routed-model\r\nX-Codex-Turn-State: state_1\r\n",
            body,
        );
        let input = vec![json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Reply ok"}]
        })];
        let mut request = Request::new("gpt-test", input);
        request.parallel_tool_calls = true;

        let mut events = stream_from(
            &Client::new(),
            Credentials {
                access_token: &crate::SecretString::from("token"),
                account_id: &crate::SecretString::from("account"),
            },
            &request,
            &endpoint,
        )
        .await
        .unwrap();

        assert_eq!(events.request_id.as_deref(), Some("req_123"));
        assert_eq!(events.model.as_deref(), Some("routed-model"));
        assert_eq!(events.turn_state.as_deref(), Some("state_1"));
        assert_eq!(
            events.next().await.unwrap().unwrap().kind,
            "response.created"
        );
        let delta = events.next().await.unwrap().unwrap();
        assert_eq!(delta.kind, "response.output_text.delta");
        assert_eq!(delta.fields["delta"], "ok");
        assert_eq!(
            events.next().await.unwrap().unwrap().kind,
            "response.completed"
        );
        assert!(events.next().await.unwrap().is_none());

        let request = server.join().unwrap();
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let headers = str::from_utf8(&request[..header_end])
            .unwrap()
            .to_ascii_lowercase();
        let body: Value = serde_json::from_slice(&request[header_end + 4..]).unwrap();

        assert!(headers.starts_with("post /responses http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer token\r\n"));
        assert!(headers.contains("\r\nchatgpt-account-id: account\r\n"));
        assert!(headers.contains("\r\naccept: text/event-stream\r\n"));
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"], Value::Null);
        assert_eq!(body["parallel_tool_calls"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert!(body.get("instructions").is_none());
        assert!(body.get("tools").is_none());
    }

    #[tokio::test]
    async fn preserves_unsuccessful_response() {
        let body = r#"{"error":{"message":"denied"}}"#;
        let (endpoint, server) = serve_once("429 Too Many Requests", "", body);
        let request = Request::new("gpt-test", Vec::new());

        let error = stream_from(
            &Client::new(),
            Credentials {
                access_token: &crate::SecretString::from("secret-token"),
                account_id: &crate::SecretString::from("account"),
            },
            &request,
            &endpoint,
        )
        .await
        .unwrap_err();
        server.join().unwrap();

        match &error {
            Error::Response { status, body: raw } => {
                assert_eq!(*status, StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(&**raw, body);
            }
            error => panic!("unexpected error: {error}"),
        }
        assert!(!error.to_string().contains(body));
        assert!(!error.to_string().contains("secret-token"));
    }
}
