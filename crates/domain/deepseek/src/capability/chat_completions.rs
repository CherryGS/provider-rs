use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Credentials;

const ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const USER_AGENT: &str = concat!("provider-deepseek/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    User {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<Value>>,
    },
    Tool {
        content: String,
        tool_call_id: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
            name: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl Request {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            thinking: None,
            reasoning_effort: None,
            max_tokens: None,
            response_format: None,
            stop: None,
            temperature: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            logprobs: None,
            top_logprobs: None,
            user_id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: ThinkingType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingType {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    High,
    Max,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub choices: Vec<Choice>,
    pub created: u64,
    pub model: String,
    pub object: String,
    #[serde(default)]
    pub system_fingerprint: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub finish_reason: String,
    pub index: u64,
    pub message: AssistantMessage,
    #[serde(default)]
    pub logprobs: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub completion_tokens: u64,
    pub prompt_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u64>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct CompletionTokenDetails {
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
                write!(
                    formatter,
                    "DeepSeek Chat Completions field `{field}` is invalid"
                )
            }
            Self::Exchange(_) => formatter.write_str("DeepSeek Chat Completions request failed"),
            Self::Response { status, .. } => {
                write!(
                    formatter,
                    "DeepSeek Chat Completions returned HTTP {status}"
                )
            }
            Self::Decode { .. } => {
                formatter.write_str("DeepSeek Chat Completions returned invalid JSON")
            }
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
        .bearer_auth(credentials.api_key)
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
        return Err(Error::InvalidCredentials("api_key"));
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if request.messages.is_empty() {
        return Err(Error::InvalidRequest("messages"));
    }
    if request.max_tokens == Some(0) {
        return Err(Error::InvalidRequest("max_tokens"));
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
    if request.top_logprobs.is_some_and(|value| value > 20)
        || (request.top_logprobs.is_some() && request.logprobs != Some(true))
    {
        return Err(Error::InvalidRequest("top_logprobs"));
    }
    if request.user_id.as_deref().is_some_and(|value| {
        value.is_empty()
            || value.len() > 512
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return Err(Error::InvalidRequest("user_id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_reasoning_controls_and_decodes_reasoning() {
        let response_body = r#"{"id":"chatcmpl-1","choices":[{"finish_reason":"stop","index":0,"message":{"content":"4","reasoning_content":"2 + 2","role":"assistant"},"logprobs":null}],"created":1786348800,"model":"deepseek-v4-pro","system_fingerprint":"fp_1","object":"chat.completion","usage":{"completion_tokens":3,"prompt_tokens":5,"prompt_cache_hit_tokens":2,"prompt_cache_miss_tokens":3,"total_tokens":8,"completion_tokens_details":{"reasoning_tokens":2}}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "deepseek-v4-pro",
            vec![
                Message::system("Answer concisely."),
                Message::user("What is 2 + 2?"),
            ],
        );
        request.thinking = Some(Thinking {
            kind: ThinkingType::Enabled,
        });
        request.reasoning_effort = Some(ReasoningEffort::High);
        request.max_tokens = Some(64);

        let response = create_at(
            &Client::new(),
            Credentials::new("test-key"),
            &request,
            &format!("{base_url}/chat/completions"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(
            response.choices[0].message.reasoning_content.as_deref(),
            Some("2 + 2")
        );
        assert_eq!(
            response
                .usage
                .as_ref()
                .and_then(|usage| usage.completion_tokens_details.as_ref())
                .and_then(|details| details.reasoning_tokens),
            Some(2)
        );

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /chat/completions http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"model":"deepseek-v4-pro","messages":[{"role":"system","content":"Answer concisely."},{"role":"user","content":"What is 2 + 2?"}],"thinking":{"type":"enabled"},"reasoning_effort":"high","max_tokens":64}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"invalid request"}}"#;
        let (base_url, requests) = serve("422 Unprocessable Entity", body);

        let error = create_at(
            &Client::new(),
            Credentials::new("test-key"),
            &Request::new("deepseek-v4-flash", vec![Message::user("hello")]),
            &format!("{base_url}/chat/completions"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNPROCESSABLE_ENTITY));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn rejects_invalid_user_id_before_exchange() {
        let mut request = Request::new("deepseek-v4-flash", vec![Message::user("hello")]);
        request.user_id = Some("private user".to_owned());

        let error = call(&Client::new(), Credentials::new("test-key"), &request)
            .await
            .expect_err("request is invalid");

        assert!(matches!(error, Error::InvalidRequest("user_id")));
    }
}
