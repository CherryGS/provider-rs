//! Anthropic model discovery.
//!
//! Endpoint behavior follows `anthropics/anthropic-sdk-typescript`
//! sdk-v0.116.0 `src/resources/models.ts` and the official
//! [Models reference](https://platform.claude.com/docs/en/api/models/list).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Credentials;

const ENDPOINT: &str = "https://api.anthropic.com/v1/models";
const API_VERSION: &str = "2023-06-01";
const USER_AGENT: &str = concat!("provider-anthropic/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub data: Vec<Model>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub capabilities: Option<Value>,
    pub created_at: String,
    pub display_name: String,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(rename = "type")]
    pub kind: String,
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
                write!(formatter, "Anthropic model-list field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Anthropic model-list request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Anthropic model list returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("Anthropic model list returned invalid JSON")
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
    list_at(client, credentials, request, ENDPOINT).await
}

async fn list_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let response = client
        .get(endpoint)
        .header("x-api-key", credentials.api_key)
        .header("anthropic-version", API_VERSION)
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .query(request)
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
    if request
        .after_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("after_id"));
    }
    if request
        .before_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("before_id"));
    }
    if request
        .limit
        .is_some_and(|limit| !(1..=1000).contains(&limit))
    {
        return Err(Error::InvalidRequest("limit"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn lists_a_page_of_models() {
        let body = r#"{"data":[{"id":"claude-test","capabilities":null,"created_at":"2026-01-01T00:00:00Z","display_name":"Claude Test","max_input_tokens":200000,"max_tokens":8192,"type":"model","preview":true}],"has_more":true,"first_id":"claude-test","last_id":"claude-test"}"#;
        let (base_url, requests) = serve("200 OK", body);
        let request = Request {
            after_id: Some("claude-previous".to_owned()),
            before_id: None,
            limit: Some(5),
        };

        let response = list_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &request,
            &format!("{base_url}/v1/models"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.data[0].id, "claude-test");
        assert_eq!(response.data[0].max_input_tokens, Some(200_000));
        assert_eq!(
            response.data[0].extra.get("preview"),
            Some(&Value::Bool(true))
        );
        assert!(response.has_more);
        assert_eq!(response.last_id.as_deref(), Some("claude-test"));

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(
            headers.starts_with("get /v1/models?after_id=claude-previous&limit=5 http/1.1\r\n")
        );
        assert!(headers.contains("\r\nx-api-key: test-key\r\n"));
        assert!(headers.contains("\r\nanthropic-version: 2023-06-01\r\n"));
        assert!(request_body.is_empty());
    }

    #[tokio::test]
    async fn rejects_an_out_of_range_limit() {
        let error = list_at(
            &Client::new(),
            Credentials {
                api_key: "test-key",
            },
            &Request {
                limit: Some(0),
                ..Request::default()
            },
            "http://127.0.0.1:1/v1/models",
        )
        .await
        .expect_err("request fails before exchange");

        assert!(matches!(error, Error::InvalidRequest("limit")));
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"bad"}}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = list_at(
            &Client::new(),
            Credentials { api_key: "bad-key" },
            &Request::default(),
            &format!("{base_url}/v1/models"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
