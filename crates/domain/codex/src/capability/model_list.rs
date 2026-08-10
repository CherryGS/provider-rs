//! Codex model discovery through the ChatGPT backend.
//!
//! Endpoint behavior follows `openai/codex` files
//! `codex-rs/codex-api/src/endpoint/models.rs`,
//! `codex-rs/model-provider/src/models_endpoint.rs`, and
//! `codex-rs/protocol/src/openai_models.rs` at pinned commit
//! `c9c6c0daa994109cec50fddcb57d076fdf9e738c`:
//! <https://github.com/openai/codex/tree/c9c6c0daa994109cec50fddcb57d076fdf9e738c>.

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/models";
const USER_AGENT: &str = concat!("provider-codex/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy)]
pub struct Credentials<'a> {
    pub access_token: &'a str,
    pub account_id: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Request {
    pub client_version: String,
}

impl Request {
    pub fn new(client_version: impl Into<String>) -> Self {
        Self {
            client_version: client_version.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub models: Vec<Model>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub default_reasoning_level: Option<String>,
    pub visibility: String,
    pub supported_in_api: bool,
    pub priority: i32,
    #[serde(default)]
    pub context_window: Option<i64>,
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
            Self::InvalidCredentials(field) => write!(formatter, "Codex {field} is empty"),
            Self::InvalidRequest(field) => {
                write!(formatter, "Codex model-list field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Codex model-list request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Codex model list returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("Codex model list returned invalid JSON"),
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
        .bearer_auth(credentials.access_token)
        .header("ChatGPT-Account-Id", credentials.account_id)
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
    if credentials.access_token.trim().is_empty() {
        return Err(Error::InvalidCredentials("access token"));
    }
    if credentials.account_id.trim().is_empty() {
        return Err(Error::InvalidCredentials("account ID"));
    }
    if request.client_version.trim().is_empty() {
        return Err(Error::InvalidRequest("client_version"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
    };

    use reqwest::StatusCode;

    use super::*;

    fn serve_once(status: &str, body: &str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let endpoint = format!(
            "http://{}/backend-api/codex/models",
            listener.local_addr().expect("test server address")
        );
        let status = status.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }

            write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write response");

            String::from_utf8(request).expect("UTF-8 request")
        });
        (endpoint, handle)
    }

    #[tokio::test]
    async fn lists_compatible_models() {
        let body = r#"{"models":[{"slug":"gpt-test","display_name":"GPT Test","description":"Test model","default_reasoning_level":"medium","visibility":"list","supported_in_api":true,"priority":1,"context_window":272000,"shell_type":"shell_command"}],"catalog_version":2}"#;
        let (endpoint, server) = serve_once("200 OK", body);

        let response = list_at(
            &Client::new(),
            Credentials {
                access_token: "test-token",
                account_id: "test-account",
            },
            &Request::new("0.99.0"),
            &endpoint,
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.models[0].slug, "gpt-test");
        assert!(response.models[0].supported_in_api);
        assert_eq!(response.models[0].context_window, Some(272_000));
        assert_eq!(
            response.models[0].extra.get("shell_type"),
            Some(&Value::String("shell_command".to_owned()))
        );
        assert_eq!(response.extra.get("catalog_version"), Some(&Value::from(2)));

        let request = server.join().expect("test server completes");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(
            headers.starts_with("get /backend-api/codex/models?client_version=0.99.0 http/1.1\r\n")
        );
        assert!(headers.contains("\r\nauthorization: bearer test-token\r\n"));
        assert!(headers.contains("\r\nchatgpt-account-id: test-account\r\n"));
        assert!(request_body.is_empty());
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"detail":"unauthorized"}"#;
        let (endpoint, server) = serve_once("401 Unauthorized", body);

        let error = list_at(
            &Client::new(),
            Credentials {
                access_token: "bad-token",
                account_id: "test-account",
            },
            &Request::new("0.99.0"),
            &endpoint,
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        server.join().expect("test server completes");
    }
}
