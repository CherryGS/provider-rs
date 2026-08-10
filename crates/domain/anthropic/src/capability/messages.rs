//! Non-streaming Anthropic Messages creation.
//!
//! Endpoint behavior follows `anthropics/anthropic-sdk-typescript`
//! sdk-v0.116.0 `src/resources/messages/messages.ts` and the official
//! [Messages reference](https://platform.claude.com/docs/en/api/messages).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Credentials;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = concat!("provider-anthropic/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<Value>),
}

impl From<String> for Content {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Content {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InputMessage {
    pub role: Role,
    pub content: Content,
}

impl InputMessage {
    pub fn user(content: impl Into<Content>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<Content>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub max_tokens: u64,
    pub messages: Vec<InputMessage>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_geo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
}

impl Request {
    pub fn new(model: impl Into<String>, max_tokens: u64, messages: Vec<InputMessage>) -> Self {
        Self {
            max_tokens,
            messages,
            model: model.into(),
            inference_geo: None,
            metadata: None,
            output_config: None,
            service_tier: None,
            stop_sequences: None,
            system: None,
            thinking: None,
            tool_choice: None,
            tools: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub content: Vec<Value>,
    pub model: String,
    pub role: String,
    #[serde(default)]
    pub stop_details: Option<Value>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(rename = "type")]
    pub object: String,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub inference_geo: Option<String>,
    #[serde(default)]
    pub output_tokens_details: Option<Value>,
    #[serde(default)]
    pub server_tool_use: Option<Value>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials,
    InvalidRequest(&'static str),
    Exchange(reqwest::Error),
    Response {
        status: StatusCode,
        body: String,
    },
    Decode {
        source: serde_json::Error,
        body: String,
    },
}

impl Error {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Response { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn raw_body(&self) -> Option<&str> {
        match self {
            Self::Response { body, .. } | Self::Decode { body, .. } => Some(body),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => formatter.write_str("Anthropic API key is empty"),
            Self::InvalidRequest(field) => {
                write!(formatter, "Anthropic Messages field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Anthropic Messages request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Anthropic Messages returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("Anthropic Messages returned invalid JSON"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Exchange(source) => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub async fn call(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
) -> Result<Response, Error> {
    create_at(client, credentials, request, ENDPOINT).await
}

async fn create_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let response = client
        .post(endpoint)
        .header("x-api-key", credentials.api_key)
        .header("anthropic-version", API_VERSION)
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .json(request)
        .send()
        .await
        .map_err(Error::Exchange)?;
    let status = response.status();
    let body = response.bytes().await.map_err(Error::Exchange)?;
    if !status.is_success() {
        return Err(Error::Response {
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }

    serde_json::from_slice(&body).map_err(|source| Error::Decode {
        source,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn validate(credentials: Credentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.trim().is_empty() {
        return Err(Error::InvalidCredentials);
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if request.messages.is_empty() {
        return Err(Error::InvalidRequest("messages"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_versioned_request_and_decodes_message() {
        let response_body = r#"{"id":"msg_1","content":[{"type":"text","text":"Hello"},{"type":"future_block","value":1}],"model":"claude-test","role":"assistant","stop_details":null,"stop_reason":"end_turn","stop_sequence":null,"type":"message","usage":{"input_tokens":8,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"inference_geo":"us","output_tokens_details":null,"server_tool_use":null,"service_tier":"standard"}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "claude-test",
            64,
            vec![InputMessage::user(Content::Blocks(vec![json!({
                "type": "text",
                "text": "Hi"
            })]))],
        );
        request.system = Some(Content::Text("Be brief".to_owned()));
        request.thinking = Some(json!({"type": "adaptive"}));

        let response = create_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &request,
            &format!("{base_url}/v1/messages"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.id, "msg_1");
        assert_eq!(response.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(response.content[1]["type"], "future_block");
        assert_eq!(response.usage.output_tokens, 2);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/messages http/1.1\r\n"));
        assert!(headers.contains("\r\nx-api-key: test-key\r\n"));
        assert!(headers.contains("\r\nanthropic-version: 2023-06-01\r\n"));
        assert_eq!(
            body,
            r#"{"max_tokens":64,"messages":[{"role":"user","content":[{"text":"Hi","type":"text"}]}],"model":"claude-test","system":"Be brief","thinking":{"type":"adaptive"}}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body =
            r#"{"type":"error","error":{"type":"authentication_error","message":"bad key"}}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = create_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &Request::new("claude-test", 8, vec![InputMessage::user("hello")]),
            &format!("{base_url}/v1/messages"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
