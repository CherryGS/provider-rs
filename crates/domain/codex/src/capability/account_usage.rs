//! Stateless Codex account usage through the ChatGPT backend.
//!
//! Endpoint and payload behavior follow `openai/codex` files
//! `codex-rs/backend-client/src/client/rate_limit_resets.rs` and
//! `codex-rs/codex-backend-openapi-models/src/models/` at pinned commit
//! `c9c6c0daa994109cec50fddcb57d076fdf9e738c`:
//! <https://github.com/openai/codex/tree/c9c6c0daa994109cec50fddcb57d076fdf9e738c>.

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{error, fmt};

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const USER_AGENT: &str = concat!("provider-codex/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy)]
pub struct Credentials<'a> {
    pub access_token: &'a str,
    pub account_id: &'a str,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Usage {
    pub plan_type: String,
    pub rate_limit: Option<RateLimit>,
    pub credits: Option<Credits>,
    pub spend_control: Option<SpendControl>,
    pub additional_rate_limits: Option<Vec<AdditionalRateLimit>>,
    pub rate_limit_reached_type: Option<RateLimitReachedType>,
    pub rate_limit_reset_credits: Option<RateLimitResetCredits>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimit {
    pub allowed: bool,
    pub limit_reached: bool,
    pub primary_window: Option<RateLimitWindow>,
    pub secondary_window: Option<RateLimitWindow>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitWindow {
    pub used_percent: i32,
    pub limit_window_seconds: i64,
    pub reset_after_seconds: i64,
    pub reset_at: i64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Credits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct SpendControl {
    pub reached: bool,
    pub individual_limit: Option<SpendControlLimit>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct SpendControlLimit {
    pub source: Option<String>,
    pub limit: String,
    pub used: String,
    pub remaining: String,
    pub used_percent: i32,
    pub remaining_percent: i32,
    pub reset_after_seconds: i64,
    pub reset_at: i64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct AdditionalRateLimit {
    pub limit_name: String,
    pub metered_feature: String,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitReachedType {
    #[serde(rename = "type")]
    pub kind: RateLimitReachedKind,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitReachedKind {
    RateLimitReached,
    WorkspaceOwnerCreditsDepleted,
    WorkspaceMemberCreditsDepleted,
    WorkspaceOwnerUsageLimitReached,
    WorkspaceMemberUsageLimitReached,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitResetCredits {
    pub available_count: i64,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
    Request(reqwest::Error),
    Response { status: StatusCode, body: Box<str> },
    Decode(reqwest::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials(field) => write!(formatter, "Codex {field} is empty"),
            Self::Request(error) => write!(formatter, "Codex usage request failed: {error}"),
            Self::Response { status, .. } => {
                write!(formatter, "Codex usage request returned {status}")
            }
            Self::Decode(error) => write!(formatter, "invalid Codex usage response: {error}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Request(error) | Self::Decode(error) => Some(error),
            Self::InvalidCredentials(_) | Self::Response { .. } => None,
        }
    }
}

pub async fn call(client: &Client, credentials: Credentials<'_>) -> Result<Usage, Error> {
    fetch_from(client, credentials, ENDPOINT).await
}

async fn fetch_from(
    client: &Client,
    credentials: Credentials<'_>,
    endpoint: &str,
) -> Result<Usage, Error> {
    if credentials.access_token.is_empty() {
        return Err(Error::InvalidCredentials("access token"));
    }
    if credentials.account_id.is_empty() {
        return Err(Error::InvalidCredentials("account ID"));
    }

    let response = client
        .get(endpoint)
        .bearer_auth(credentials.access_token)
        .header("ChatGPT-Account-Id", credentials.account_id)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(Error::Request)?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(Error::Decode)?
            .into_boxed_str();
        return Err(Error::Response { status, body });
    }

    response.json().await.map_err(Error::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
    };

    fn serve_once(status: &str, body: &str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/usage", listener.local_addr().unwrap());
        let status = status.to_owned();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }

            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();

            String::from_utf8(request).unwrap()
        });
        (endpoint, handle)
    }

    #[tokio::test]
    async fn fetches_usage() {
        let body = r#"{
            "plan_type": "plus",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 42,
                    "limit_window_seconds": 18000,
                    "reset_after_seconds": 900,
                    "reset_at": 1770000000
                },
                "secondary_window": null
            },
            "credits": {
                "has_credits": true,
                "unlimited": false,
                "balance": "12.50"
            },
            "spend_control": {
                "reached": false,
                "individual_limit": {
                    "source": "user",
                    "limit": "20",
                    "used": "7.5",
                    "remaining": "12.5",
                    "used_percent": 38,
                    "remaining_percent": 62,
                    "reset_after_seconds": 86400,
                    "reset_at": 1770000000
                }
            },
            "additional_rate_limits": [{
                "limit_name": "Review",
                "metered_feature": "reviews",
                "rate_limit": null
            }],
            "rate_limit_reached_type": {"type": "unknown"},
            "rate_limit_reset_credits": {"available_count": 2}
        }"#;
        let (endpoint, server) = serve_once("200 OK", body);

        let usage = fetch_from(
            &Client::new(),
            Credentials {
                access_token: "token",
                account_id: "account",
            },
            &endpoint,
        )
        .await
        .unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();

        assert!(request.starts_with("get /usage http/1.1\r\n"));
        assert!(request.contains("\r\nauthorization: bearer token\r\n"));
        assert!(request.contains("\r\nchatgpt-account-id: account\r\n"));
        assert!(request.contains("\r\nuser-agent: provider-codex/0.1.0\r\n"));
        assert_eq!(usage.plan_type, "plus");
        assert_eq!(
            usage
                .rate_limit
                .unwrap()
                .primary_window
                .unwrap()
                .used_percent,
            42
        );
        assert_eq!(usage.credits.unwrap().balance.as_deref(), Some("12.50"));
        assert_eq!(usage.rate_limit_reset_credits.unwrap().available_count, 2);
    }

    #[tokio::test]
    async fn preserves_unsuccessful_response() {
        let (endpoint, server) = serve_once("401 Unauthorized", r#"{"detail":"unauthorized"}"#);

        let error = fetch_from(
            &Client::new(),
            Credentials {
                access_token: "bad-token",
                account_id: "account",
            },
            &endpoint,
        )
        .await
        .unwrap_err();
        server.join().unwrap();

        match error {
            Error::Response { status, body } => {
                assert_eq!(status, StatusCode::UNAUTHORIZED);
                assert_eq!(&*body, r#"{"detail":"unauthorized"}"#);
            }
            error => panic!("unexpected error: {error}"),
        }
    }
}
