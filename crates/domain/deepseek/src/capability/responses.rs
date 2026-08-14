//! Non-streaming, stateless DeepSeek Responses creation.
//!
//! Endpoint behavior follows the official
//! [Responses API](https://api-docs.deepseek.com/api/create-response).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.deepseek.com/responses";
const USER_AGENT: &str = concat!("provider-deepseek/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Input {
    Text(String),
    Items(Vec<Value>),
}

impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Input {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl Request {
    pub fn new(model: impl Into<String>, input: impl Into<Input>) -> Self {
        Self {
            model: model.into(),
            input: Some(input.into()),
            instructions: None,
            reasoning: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            text: None,
            tools: None,
            tool_choice: None,
            top_logprobs: None,
            user: None,
        }
    }

    pub fn with_instructions(model: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            input: None,
            instructions: Some(instructions.into()),
            reasoning: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            text: None,
            tools: None,
            tool_choice: None,
            top_logprobs: None,
            user: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Reasoning {
    pub effort: ReasoningEffort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Text {
    pub format: TextFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextFormat {
    Text,
    JsonObject,
    JsonSchema { name: String, schema: Value },
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub status: String,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub incomplete_details: Option<Value>,
    pub model: String,
    pub output: Vec<Value>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokenDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokenDetails>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct InputTokenDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct OutputTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
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
            Self::InvalidCredentials(field) => {
                write!(formatter, "DeepSeek credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(formatter, "DeepSeek Responses field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("DeepSeek Responses request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "DeepSeek Responses returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("DeepSeek Responses returned invalid JSON"),
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
        .bearer_auth(credentials.api_key.expose_secret())
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
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("api_key"));
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if request.input.is_none() && request.instructions.is_none() {
        return Err(Error::InvalidRequest("input"));
    }
    if request.input.as_ref().is_some_and(|input| match input {
        Input::Text(value) => value.trim().is_empty(),
        Input::Items(items) => items.is_empty(),
    }) {
        return Err(Error::InvalidRequest("input"));
    }
    if request
        .instructions
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("instructions"));
    }
    if request.max_output_tokens == Some(0) {
        return Err(Error::InvalidRequest("max_output_tokens"));
    }
    if request
        .temperature
        .is_some_and(|value| !(0.0..=2.0).contains(&value))
    {
        return Err(Error::InvalidRequest("temperature"));
    }
    if request
        .top_p
        .is_some_and(|value| !(0.0..=1.0).contains(&value))
    {
        return Err(Error::InvalidRequest("top_p"));
    }
    if request.top_logprobs.is_some_and(|value| value > 20) {
        return Err(Error::InvalidRequest("top_logprobs"));
    }
    if request.user.as_deref().is_some_and(|value| {
        value.is_empty()
            || value.len() > 512
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return Err(Error::InvalidRequest("user"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider_test_support::serve_json as serve;

    #[tokio::test]
    async fn sends_supported_fields_and_decodes_reasoning_usage() {
        let response_body = r#"{"id":"resp_1","object":"response","created_at":1786348800,"status":"completed","error":null,"incomplete_details":null,"model":"deepseek-v4-flash","output":[{"type":"reasoning","id":"rs_1","status":"completed","content":[{"type":"reasoning_text","text":"2 + 2"}]},{"type":"message","id":"msg_1","status":"completed","role":"assistant","content":[{"type":"output_text","text":"4"}]}],"usage":{"input_tokens":5,"input_tokens_details":{"cached_tokens":2},"output_tokens":3,"output_tokens_details":{"reasoning_tokens":2},"total_tokens":8},"store":false,"previous_response_id":null,"parallel_tool_calls":true}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new("deepseek-v4-flash", "What is 2 + 2?");
        request.instructions = Some("Answer concisely.".to_owned());
        request.reasoning = Some(Reasoning {
            effort: ReasoningEffort::High,
        });
        request.max_output_tokens = Some(64);
        request.text = Some(Text {
            format: TextFormat::JsonSchema {
                name: "answer".to_owned(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {"answer": {"type": "integer"}},
                    "required": ["answer"],
                    "additionalProperties": false
                }),
            },
        });

        let response = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &request,
            &format!("{base_url}/responses"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.output[0]["type"], "reasoning");
        assert_eq!(
            response
                .usage
                .as_ref()
                .and_then(|usage| usage.output_tokens_details.as_ref())
                .and_then(|details| details.reasoning_tokens),
            Some(2)
        );

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /responses http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"model":"deepseek-v4-flash","input":"What is 2 + 2?","instructions":"Answer concisely.","reasoning":{"effort":"high"},"max_output_tokens":64,"text":{"format":{"type":"json_schema","name":"answer","schema":{"additionalProperties":false,"properties":{"answer":{"type":"integer"}},"required":["answer"],"type":"object"}}}}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"context too long"}}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::new("deepseek-v4-flash", "hello"),
            &format!("{base_url}/responses"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
