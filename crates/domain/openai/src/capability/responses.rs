//! Non-streaming OpenAI Responses creation.
//!
//! Endpoint behavior follows `openai-node` v7.4.0
//! `src/resources/responses/responses.ts` and the official
//! [Responses reference](https://platform.openai.com/docs/api-reference/responses/create).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.openai.com/v1/responses";
const USER_AGENT: &str = concat!("provider-openai/", env!("CARGO_PKG_VERSION"));

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
    pub input: Input,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
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
    pub fn new(model: impl Into<String>, input: impl Into<Input>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            instructions: None,
            max_output_tokens: None,
            metadata: None,
            parallel_tool_calls: None,
            previous_response_id: None,
            reasoning: None,
            service_tier: None,
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
    pub object: String,
    pub created_at: i64,
    pub model: String,
    pub output: Vec<Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub incomplete_details: Option<Value>,
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
    pub input_tokens_details: Option<Value>,
    #[serde(default)]
    pub output_tokens_details: Option<Value>,
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
                write!(formatter, "OpenAI Responses field `{field}` is empty")
            }
            Self::Exchange(_) => formatter.write_str("OpenAI Responses request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "OpenAI Responses returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("OpenAI Responses returned invalid JSON"),
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
    if matches!(&request.input, Input::Text(value) if value.trim().is_empty()) {
        return Err(Error::InvalidRequest("input"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::*;
    use provider_test_support::serve_json as serve;

    #[tokio::test]
    async fn sends_scope_and_decodes_response() {
        let response_body = r#"{"id":"resp_1","object":"response","created_at":1786320000,"model":"gpt-test","status":"completed","output":[{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":4,"output_tokens":2,"total_tokens":6,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}},"new_field":true}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new("gpt-test", "Say hi");
        request.instructions = Some("Be brief".to_owned());
        request.max_output_tokens = Some(32);
        request.store = Some(false);

        let response = create_at(
            &Client::new(),
            Credentials {
                api_key: &crate::SecretString::from("test-key"),
                organization: Some(&crate::SecretString::from("org-1")),
                project: Some(&crate::SecretString::from("proj-1")),
            },
            &request,
            &format!("{base_url}/v1/responses"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.id, "resp_1");
        assert_eq!(response.status.as_deref(), Some("completed"));
        assert_eq!(response.usage.expect("usage").total_tokens, 6);
        assert_eq!(response.extra.get("new_field"), Some(&json!(true)));

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/responses http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert!(headers.contains("\r\nopenai-organization: org-1\r\n"));
        assert!(headers.contains("\r\nopenai-project: proj-1\r\n"));
        assert_eq!(
            body,
            r#"{"model":"gpt-test","input":"Say hi","instructions":"Be brief","max_output_tokens":32,"store":false}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"denied"}}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::new("gpt-test", "hello"),
            &format!("{base_url}/v1/responses"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
