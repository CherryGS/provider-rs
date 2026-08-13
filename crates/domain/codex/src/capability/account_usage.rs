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

pub use crate::Credentials;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const USER_AGENT: &str = concat!("provider-codex/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Usage {
    pub plan_type: String,
    pub rate_limit: Option<RateLimit>,
    pub credits: Option<Credits>,
    pub spend_control: Option<SpendControl>,
    pub additional_rate_limits: Option<Vec<AdditionalRateLimit>>,
    pub rate_limit_reached_type: Option<RateLimitReachedType>,
    pub rate_limit_reset_credits: Option<RateLimitResetCredits>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimit {
    pub allowed: bool,
    pub limit_reached: bool,
    pub primary_window: Option<RateLimitWindow>,
    pub secondary_window: Option<RateLimitWindow>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitWindow {
    pub used_percent: i32,
    pub limit_window_seconds: i64,
    pub reset_after_seconds: i64,
    pub reset_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowError {
    PercentageOutOfRange,
    InvalidDuration,
    InvalidReset,
    TimestampOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InferredPeriod {
    pub inferred_start_at_unix_seconds: i64,
    pub reset_at_unix_seconds: i64,
}

impl RateLimitWindow {
    pub fn used_percent(&self) -> Result<u8, WindowError> {
        checked_percent(self.used_percent)
    }

    pub fn remaining_percent(&self) -> Result<u8, WindowError> {
        Ok(100 - self.used_percent()?)
    }

    pub const fn reset_at_unix_seconds(&self) -> i64 {
        self.reset_at
    }

    pub const fn reset_after_seconds(&self) -> i64 {
        self.reset_after_seconds
    }

    /// Infers a full window from Codex's reported duration and reset fields.
    ///
    /// When `reset_at` is absent or invalid, the supplied observation time and
    /// `reset_after_seconds` provide the reset. The inferred start is named as
    /// such because Codex reports no observed start timestamp. Field meaning is
    /// sourced from `openai/codex` at the commit linked in this module's docs.
    pub fn inferred_period_at(
        &self,
        observed_at_unix_seconds: i64,
    ) -> Result<InferredPeriod, WindowError> {
        if self.limit_window_seconds <= 0 {
            return Err(WindowError::InvalidDuration);
        }
        let reset_at = if self.reset_at > 0 {
            self.reset_at
        } else {
            if observed_at_unix_seconds <= 0 || self.reset_after_seconds < 0 {
                return Err(WindowError::InvalidReset);
            }
            observed_at_unix_seconds
                .checked_add(self.reset_after_seconds)
                .ok_or(WindowError::TimestampOverflow)?
        };
        let start_at = reset_at
            .checked_sub(self.limit_window_seconds)
            .ok_or(WindowError::TimestampOverflow)?;
        if start_at < 0 {
            return Err(WindowError::InvalidReset);
        }
        Ok(InferredPeriod {
            inferred_start_at_unix_seconds: start_at,
            reset_at_unix_seconds: reset_at,
        })
    }
}

pub fn checked_percent(percent: i32) -> Result<u8, WindowError> {
    u8::try_from(percent)
        .ok()
        .filter(|percent| *percent <= 100)
        .ok_or(WindowError::PercentageOutOfRange)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Credits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreditState<'a> {
    Unavailable,
    Empty,
    Balance(&'a str),
    Unlimited,
}

impl Usage {
    pub fn credit_state(&self) -> CreditState<'_> {
        self.credits
            .as_ref()
            .map_or(CreditState::Unavailable, Credits::state)
    }
}

impl Credits {
    pub fn state(&self) -> CreditState<'_> {
        if self.unlimited {
            CreditState::Unlimited
        } else if !self.has_credits {
            CreditState::Empty
        } else {
            self.balance
                .as_deref()
                .map_or(CreditState::Unavailable, CreditState::Balance)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SpendControl {
    pub reached: bool,
    pub individual_limit: Option<SpendControlLimit>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

impl SpendControlLimit {
    pub fn used_percent(&self) -> Result<u8, WindowError> {
        checked_percent(self.used_percent)
    }

    pub fn remaining_percent(&self) -> Result<u8, WindowError> {
        checked_percent(self.remaining_percent)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdditionalRateLimit {
    pub limit_name: String,
    pub metered_feature: String,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowKind {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowScope<'a> {
    Default,
    Additional {
        limit_name: &'a str,
        metered_feature: &'a str,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct WindowRef<'a> {
    pub scope: WindowScope<'a>,
    pub kind: WindowKind,
    pub window: &'a RateLimitWindow,
}

impl Usage {
    pub fn rate_limit_windows(&self) -> impl Iterator<Item = WindowRef<'_>> {
        self.rate_limit
            .iter()
            .flat_map(|limit| windows(WindowScope::Default, limit))
            .chain(
                self.additional_rate_limits
                    .iter()
                    .flatten()
                    .filter_map(|additional| {
                        additional.rate_limit.as_ref().map(|limit| {
                            (
                                WindowScope::Additional {
                                    limit_name: &additional.limit_name,
                                    metered_feature: &additional.metered_feature,
                                },
                                limit,
                            )
                        })
                    })
                    .flat_map(|(scope, limit)| windows(scope, limit)),
            )
    }
}

fn windows<'a>(
    scope: WindowScope<'a>,
    limit: &'a RateLimit,
) -> impl Iterator<Item = WindowRef<'a>> {
    [
        (WindowKind::Primary, limit.primary_window.as_ref()),
        (WindowKind::Secondary, limit.secondary_window.as_ref()),
    ]
    .into_iter()
    .filter_map(move |(kind, window)| {
        window.map(|window| WindowRef {
            scope,
            kind,
            window,
        })
    })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitReachedType {
    #[serde(rename = "type")]
    pub kind: RateLimitReachedKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RateLimitResetCredits {
    pub available_count: i64,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
    Request(reqwest::Error),
    Response {
        status: StatusCode,
        body: Box<str>,
    },
    Decode {
        source: serde_json::Error,
        body: Box<str>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidCredentials,
    Transport,
    HttpResponse,
    Decode,
}

impl Error {
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidCredentials(_) => ErrorKind::InvalidCredentials,
            Self::Request(_) => ErrorKind::Transport,
            Self::Response { .. } => ErrorKind::HttpResponse,
            Self::Decode { .. } => ErrorKind::Decode,
        }
    }

    pub const fn status(&self) -> Option<StatusCode> {
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
            Self::Request(_) => formatter.write_str("Codex usage request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Codex usage request returned {status}")
            }
            Self::Decode { .. } => formatter.write_str("invalid Codex usage response"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Request(error) => Some(error),
            Self::Decode { source, .. } => Some(source),
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
            .map_err(Error::Request)?
            .into_boxed_str();
        return Err(Error::Response { status, body });
    }

    let body = response.bytes().await.map_err(Error::Request)?;
    serde_json::from_slice(&body).map_err(|source| Error::Decode {
        source,
        body: String::from_utf8_lossy(&body).into_owned().into_boxed_str(),
    })
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

        assert_eq!(error.kind(), ErrorKind::HttpResponse);
        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(r#"{"detail":"unauthorized"}"#));
        assert!(!error.to_string().contains("unauthorized"));
    }

    #[tokio::test]
    async fn preserves_invalid_typed_response_without_displaying_it() {
        let (endpoint, server) = serve_once("200 OK", "not JSON");

        let error = fetch_from(
            &Client::new(),
            Credentials {
                access_token: "secret-token",
                account_id: "secret-account",
            },
            &endpoint,
        )
        .await
        .unwrap_err();
        server.join().unwrap();

        assert_eq!(error.kind(), ErrorKind::Decode);
        assert_eq!(error.raw_body(), Some("not JSON"));
        assert_eq!(error.to_string(), "invalid Codex usage response");
        assert!(!format!("{error:?}").contains("secret-token"));
    }

    #[test]
    fn exposes_checked_window_and_credit_semantics() {
        let window = RateLimitWindow {
            used_percent: 42,
            limit_window_seconds: 18_000,
            reset_after_seconds: 900,
            reset_at: 0,
        };
        assert_eq!(window.remaining_percent(), Ok(58));
        assert_eq!(
            window.inferred_period_at(1_770_000_000),
            Ok(InferredPeriod {
                inferred_start_at_unix_seconds: 1_769_982_900,
                reset_at_unix_seconds: 1_770_000_900,
            })
        );
        assert_eq!(
            RateLimitWindow {
                used_percent: 101,
                ..window
            }
            .remaining_percent(),
            Err(WindowError::PercentageOutOfRange)
        );

        let spend = SpendControlLimit {
            source: None,
            limit: "100".into(),
            used: "25".into(),
            remaining: "75".into(),
            used_percent: 25,
            remaining_percent: 75,
            reset_after_seconds: 900,
            reset_at: 0,
        };
        assert_eq!(spend.used_percent(), Ok(25));
        assert_eq!(spend.remaining_percent(), Ok(75));

        for (credits, expected) in [
            (None, CreditState::Unavailable),
            (
                Some(Credits {
                    has_credits: false,
                    unlimited: false,
                    balance: None,
                }),
                CreditState::Empty,
            ),
            (
                Some(Credits {
                    has_credits: true,
                    unlimited: false,
                    balance: Some("12.50".into()),
                }),
                CreditState::Balance("12.50"),
            ),
            (
                Some(Credits {
                    has_credits: true,
                    unlimited: true,
                    balance: None,
                }),
                CreditState::Unlimited,
            ),
        ] {
            let usage = Usage {
                plan_type: "plus".into(),
                rate_limit: None,
                credits,
                spend_control: None,
                additional_rate_limits: None,
                rate_limit_reached_type: None,
                rate_limit_reset_credits: None,
            };
            assert_eq!(usage.credit_state(), expected);
        }
    }
}
