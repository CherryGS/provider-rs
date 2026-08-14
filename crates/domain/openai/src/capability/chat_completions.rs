//! Non-streaming OpenAI Chat Completions creation.
//!
//! Endpoint behavior follows `openai-node` v7.4.0
//! `src/resources/chat/completions/completions.ts` and the official
//! [Chat reference](https://platform.openai.com/docs/api-reference/chat/create).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
const USER_AGENT: &str = concat!("provider-openai/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<Value>),
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
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    Developer {
        content: Content,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    System {
        content: Content,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    User {
        content: Content,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Assistant {
        content: Option<Content>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<Value>>,
    },
    Tool {
        content: Content,
        tool_call_id: String,
    },
}

impl Message {
    pub fn developer(content: impl Into<Content>) -> Self {
        Self::Developer {
            content: content.into(),
            name: None,
        }
    }

    pub fn system(content: impl Into<Content>) -> Self {
        Self::System {
            content: content.into(),
            name: None,
        }
    }

    pub fn user(content: impl Into<Content>) -> Self {
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
    pub max_completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
}

impl Request {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            max_completion_tokens: None,
            metadata: None,
            n: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
            response_format: None,
            service_tier: None,
            stop: None,
            store: None,
            temperature: None,
            tool_choice: None,
            tools: None,
            top_p: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub choices: Vec<Choice>,
    pub created: i64,
    pub model: String,
    pub object: String,
    #[serde(default)]
    pub service_tier: Option<String>,
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
    #[serde(default)]
    pub logprobs: Option<Value>,
    pub message: AssistantMessage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub refusal: Option<String>,
    pub role: String,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<Value>,
    #[serde(default)]
    pub completion_tokens_details: Option<Value>,
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
                write!(formatter, "OpenAI credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(
                    formatter,
                    "OpenAI Chat Completions field `{field}` is empty"
                )
            }
            Self::Exchange(_) => formatter.write_str("OpenAI Chat Completions request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "OpenAI Chat Completions returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("OpenAI Chat Completions returned invalid JSON")
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

    let mut builder = client
        .post(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .json(request);
    if let Some(organization) = credentials.organization {
        builder = builder.header("OpenAI-Organization", organization.expose_secret());
    }
    if let Some(project) = credentials.project {
        builder = builder.header("OpenAI-Project", project.expose_secret());
    }

    let response = builder.send().await.map_err(Error::Exchange)?;
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
    if credentials
        .organization
        .is_some_and(|value| value.expose_secret().trim().is_empty())
    {
        return Err(Error::InvalidCredentials("organization"));
    }
    if credentials
        .project
        .is_some_and(|value| value.expose_secret().trim().is_empty())
    {
        return Err(Error::InvalidCredentials("project"));
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

    use super::*;
    use provider_test_support::serve_json as serve;

    #[tokio::test]
    async fn sends_messages_and_decodes_completion() {
        let response_body = r#"{"id":"chatcmpl_1","choices":[{"finish_reason":"stop","index":0,"logprobs":null,"message":{"content":"hello","refusal":null,"role":"assistant"}}],"created":1786320000,"model":"gpt-test","object":"chat.completion","service_tier":"default","usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":0}}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "gpt-test",
            vec![
                Message::developer("Be terse"),
                Message::User {
                    content: Content::Parts(vec![serde_json::json!({
                        "type": "text",
                        "text": "Hi"
                    })]),
                    name: None,
                },
            ],
        );
        request.max_completion_tokens = Some(16);
        request.reasoning_effort = Some("low".to_owned());
        request.store = Some(false);

        let response = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &request,
            &format!("{base_url}/v1/chat/completions"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.id, "chatcmpl_1");
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("hello")
        );
        assert_eq!(response.usage.expect("usage").total_tokens, 7);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/chat/completions http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"model":"gpt-test","messages":[{"role":"developer","content":"Be terse"},{"role":"user","content":[{"text":"Hi","type":"text"}]}],"max_completion_tokens":16,"reasoning_effort":"low","store":false}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"limited"}}"#;
        let (base_url, requests) = serve("429 Too Many Requests", body);

        let error = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::new("gpt-test", vec![Message::user("hello")]),
            &format!("{base_url}/v1/chat/completions"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
