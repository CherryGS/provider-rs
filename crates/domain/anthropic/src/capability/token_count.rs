//! Anthropic Messages input-token counting without generation.
//!
//! Endpoint behavior follows `anthropics/anthropic-sdk-typescript`
//! sdk-v0.116.0 `src/resources/messages/messages.ts` and the official
//! [token-counting reference](https://platform.claude.com/docs/en/api/messages-count-tokens).

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Credentials,
    capability::messages::{Content, InputMessage},
};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages/count_tokens";
const API_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = concat!("provider-anthropic/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub messages: Vec<InputMessage>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<Value>,
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
    pub fn new(model: impl Into<String>, messages: Vec<InputMessage>) -> Self {
        Self {
            messages,
            model: model.into(),
            output_config: None,
            system: None,
            thinking: None,
            tool_choice: None,
            tools: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub input_tokens: u64,
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
                write!(
                    formatter,
                    "Anthropic token-count field `{field}` is invalid"
                )
            }
            Self::Exchange(_) => formatter.write_str("Anthropic token-count request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Anthropic token count returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("Anthropic token count returned invalid JSON")
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
    count_at(client, credentials, request, ENDPOINT).await
}

async fn count_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    if credentials.api_key.trim().is_empty() {
        return Err(Error::InvalidCredentials);
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if request.messages.is_empty() {
        return Err(Error::InvalidRequest("messages"));
    }

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

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_message_shape_and_decodes_count() {
        let (base_url, requests) = serve("200 OK", r#"{"input_tokens":14}"#);
        let mut request = Request::new("claude-test", vec![InputMessage::user("Count this")]);
        request.system = Some(Content::Text("Be concise".to_owned()));
        request.tools = Some(vec![json!({
            "name": "lookup",
            "description": "Look up a value",
            "input_schema": {"type": "object"}
        })]);

        let response = count_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &request,
            &format!("{base_url}/v1/messages/count_tokens"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.input_tokens, 14);
        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/messages/count_tokens http/1.1\r\n"));
        assert!(headers.contains("\r\nx-api-key: test-key\r\n"));
        assert!(headers.contains("\r\nanthropic-version: 2023-06-01\r\n"));
        assert_eq!(
            body,
            r#"{"messages":[{"role":"user","content":"Count this"}],"model":"claude-test","system":"Be concise","tools":[{"description":"Look up a value","input_schema":{"type":"object"},"name":"lookup"}]}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = count_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &Request::new("claude-test", vec![InputMessage::user("hello")]),
            &format!("{base_url}/v1/messages/count_tokens"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
