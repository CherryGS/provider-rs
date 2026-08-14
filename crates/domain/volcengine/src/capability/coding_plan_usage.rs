//! Coding Plan Team seat quota usage.
//!
//! This wraps Volcengine's documented `GetSeatInfoUsage` action. The personal
//! Coding Plan currently exposes quota usage only in the console.
//! Contract: <https://www.volcengine.com/docs/82379/2306579>

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};

use super::{EndpointOutcome, ErrorKind, MillisecondPeriod};
pub use super::{ProviderError, ResponseMetadata};
use crate::{Credentials, ExposeSecret, signing};

const ACTION: &str = "GetSeatInfoUsage";
const ENDPOINT: &str =
    "https://ark.cn-beijing.volces.com/?Action=GetSeatInfoUsage&Version=2024-01-01";

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Request<'a> {
    #[serde(rename = "SeatID")]
    pub seat_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<&'a str>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Response {
    pub response_metadata: ResponseMetadata,
    pub result: Option<Usage>,
}

impl Response {
    pub fn into_outcome(self) -> EndpointOutcome<Usage> {
        EndpointOutcome::from_parts(self.result, self.response_metadata)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Usage {
    #[serde(rename = "SeatID")]
    pub seat_id: String,
    #[serde(rename = "AccountID")]
    pub account_id: u64,
    pub project_name: String,
    #[serde(rename = "UserID")]
    pub user_id: String,
    pub user_name: String,
    /// Monthly period start as a Unix timestamp in milliseconds.
    ///
    /// Source: `GetSeatInfoUsage`, updated 2026-07-31:
    /// <https://www.volcengine.com/docs/82379/2306579>
    pub monthly_subscribe_milestone: i64,
    /// Monthly reset as a Unix timestamp in milliseconds.
    pub monthly_reset_milestone: i64,
    /// Provider-reported used percentage for the five-hour window.
    pub short_term_usage: f64,
    /// Provider-reported used percentage for the weekly window.
    pub weekly_usage: f64,
    /// Provider-reported used percentage for the monthly window.
    pub monthly_usage: f64,
    /// Five-hour reset as a Unix-millisecond timestamp, or `-1` when usage is zero.
    pub short_term_reset_milestone: i64,
    /// Weekly reset as a Unix timestamp in milliseconds.
    pub weekly_reset_milestone: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowKind {
    FiveHour,
    Weekly,
    Monthly,
}

#[derive(Clone, Copy, Debug)]
pub struct UsageWindow {
    kind: WindowKind,
    used_percent: f64,
    reset_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageWindowError {
    NonFinite,
    PercentageOutOfRange,
    InvalidReset,
    InvalidMonthlyPeriod,
}

impl Usage {
    /// Iterates the percentages and reset milestones documented by
    /// `GetSeatInfoUsage`: <https://www.volcengine.com/docs/82379/2306579>.
    pub fn windows(&self) -> impl ExactSizeIterator<Item = UsageWindow> {
        [
            UsageWindow {
                kind: WindowKind::FiveHour,
                used_percent: self.short_term_usage,
                reset_at_ms: self.short_term_reset_milestone,
            },
            UsageWindow {
                kind: WindowKind::Weekly,
                used_percent: self.weekly_usage,
                reset_at_ms: self.weekly_reset_milestone,
            },
            UsageWindow {
                kind: WindowKind::Monthly,
                used_percent: self.monthly_usage,
                reset_at_ms: self.monthly_reset_milestone,
            },
        ]
        .into_iter()
    }

    pub fn monthly_period(&self) -> Result<MillisecondPeriod, UsageWindowError> {
        if self.monthly_subscribe_milestone <= 0
            || self.monthly_reset_milestone <= self.monthly_subscribe_milestone
        {
            return Err(UsageWindowError::InvalidMonthlyPeriod);
        }
        Ok(MillisecondPeriod {
            start_ms: self.monthly_subscribe_milestone,
            reset_ms: self.monthly_reset_milestone,
        })
    }
}

impl UsageWindow {
    pub const fn new(kind: WindowKind, used_percent: f64, reset_at_ms: i64) -> Self {
        Self {
            kind,
            used_percent,
            reset_at_ms,
        }
    }

    pub const fn kind(self) -> WindowKind {
        self.kind
    }

    pub fn used_percent(self) -> Result<f64, UsageWindowError> {
        if !self.used_percent.is_finite() {
            return Err(UsageWindowError::NonFinite);
        }
        if !(0.0..=100.0).contains(&self.used_percent) {
            return Err(UsageWindowError::PercentageOutOfRange);
        }
        Ok(self.used_percent)
    }

    pub fn remaining_percent(self) -> Result<f64, UsageWindowError> {
        Ok(100.0 - self.used_percent()?)
    }

    pub fn reset_at_ms(self) -> Result<Option<i64>, UsageWindowError> {
        let used_percent = self.used_percent()?;
        if self.kind == WindowKind::FiveHour && used_percent == 0.0 && self.reset_at_ms == -1 {
            return Ok(None);
        }
        if self.reset_at_ms <= 0 {
            return Err(UsageWindowError::InvalidReset);
        }
        Ok(Some(self.reset_at_ms))
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
    InvalidRequest(&'static str),
    Encode(serde_json::Error),
    Clock(time::error::Format),
    Signing,
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
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidCredentials(_) => ErrorKind::InvalidCredentials,
            Self::InvalidRequest(_) => ErrorKind::InvalidRequest,
            Self::Encode(_) => ErrorKind::Encode,
            Self::Clock(_) => ErrorKind::Clock,
            Self::Signing => ErrorKind::Signing,
            Self::Exchange(_) => ErrorKind::Transport,
            Self::Response { .. } => ErrorKind::HttpResponse,
            Self::Decode { .. } => ErrorKind::Decode,
        }
    }

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
                write!(formatter, "Volcengine credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(formatter, "Coding Plan request field `{field}` is empty")
            }
            Self::Encode(_) => formatter.write_str("failed to encode Coding Plan request"),
            Self::Clock(_) => formatter.write_str("failed to format the Volcengine signing time"),
            Self::Signing => formatter.write_str("failed to sign the Coding Plan request"),
            Self::Exchange(_) => formatter.write_str("Coding Plan request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Coding Plan request returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("Coding Plan response was not valid JSON"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Encode(source) => Some(source),
            Self::Clock(source) => Some(source),
            Self::Exchange(source) => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub async fn call(
    client: &Client,
    credentials: Credentials<'_>,
    request: Request<'_>,
) -> Result<Response, Error> {
    let x_date = signing::x_date().map_err(Error::Clock)?;
    execute(client, credentials, request, ENDPOINT, &x_date).await
}

async fn execute(
    client: &Client,
    credentials: Credentials<'_>,
    request: Request<'_>,
    endpoint: &str,
    x_date: &str,
) -> Result<Response, Error> {
    if credentials.access_key_id.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("access_key_id"));
    }
    if credentials
        .secret_access_key
        .expose_secret()
        .trim()
        .is_empty()
    {
        return Err(Error::InvalidCredentials("secret_access_key"));
    }
    if request.seat_id.trim().is_empty() {
        return Err(Error::InvalidRequest("seat_id"));
    }
    if request
        .project_name
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("project_name"));
    }

    let body = serde_json::to_vec(&request).map_err(Error::Encode)?;
    let signed = signing::sign(ACTION, &body, credentials, x_date).ok_or(Error::Signing)?;
    let response = client
        .post(endpoint)
        .header(header::HOST, signing::HOST)
        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
        .header("X-Date", x_date)
        .header("X-Content-Sha256", signed.payload_hash)
        .header(header::AUTHORIZATION, signed.authorization)
        .body(body)
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
    use provider_test_support::serve_json as serve;

    use super::*;

    const X_DATE: &str = "20260810T120000Z";

    #[tokio::test]
    async fn sends_signed_request_and_decodes_usage() {
        let access_key_id = crate::SecretString::from("AKIDEXAMPLE");
        let secret_access_key = crate::SecretString::from("secret");
        let credentials = Credentials {
            access_key_id: &access_key_id,
            secret_access_key: &secret_access_key,
        };
        let response = r#"{
            "ResponseMetadata":{"RequestId":"request-1","Action":"GetSeatInfoUsage","Version":"2024-01-01","Service":"ark","Region":"cn-beijing"},
            "Result":{"SeatID":"seat-1","AccountID":42,"ProjectName":"default","UserID":"user-1","UserName":"Ada","MonthlySubscribeMilestone":1786320000000,"MonthlyResetMilestone":1788998400000,"ShortTermUsage":12.5,"WeeklyUsage":20.0,"MonthlyUsage":30.0,"ShortTermResetMilestone":1786341600000,"WeeklyResetMilestone":1786924800000}
        }"#;
        let (base_url, requests) = serve("200 OK", response);
        let endpoint = format!("{base_url}/?Action=GetSeatInfoUsage&Version=2024-01-01");

        let response = execute(
            &Client::new(),
            credentials,
            Request {
                seat_id: "seat-1",
                project_name: Some("default"),
            },
            &endpoint,
            X_DATE,
        )
        .await
        .expect("request succeeds");

        let usage = match response.into_outcome() {
            EndpointOutcome::Success { data, .. } => data,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        assert_eq!(usage.seat_id, "seat-1");
        assert_eq!(usage.account_id, 42);
        assert_eq!(usage.short_term_usage, 12.5);
        let windows: Vec<_> = usage.windows().collect();
        assert_eq!(windows[0].kind(), WindowKind::FiveHour);
        assert_eq!(windows[0].used_percent(), Ok(12.5));
        assert_eq!(windows[0].remaining_percent(), Ok(87.5));
        assert_eq!(windows[0].reset_at_ms(), Ok(Some(1_786_341_600_000)));

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(
            headers.starts_with("post /?action=getseatinfousage&version=2024-01-01 http/1.1\r\n")
        );
        assert!(headers.contains("\r\nhost: ark.cn-beijing.volces.com\r\n"));
        assert!(headers.contains(
            "\r\nx-content-sha256: 8fd56cd70cd257bbeb841642f44c88e5ac40461762ea43d23b9392a3c852b5de\r\n"
        ));
        assert!(headers.contains(
            "\r\nauthorization: hmac-sha256 credential=akidexample/20260810/cn-beijing/ark/request, signedheaders=host;x-content-sha256;x-date, signature=7da2a8b64677ed5df397a1f995cdd071266db7741cda281101f9fb4ecb5814bd\r\n"
        ));
        assert_eq!(body, r#"{"SeatID":"seat-1","ProjectName":"default"}"#);
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let access_key_id = crate::SecretString::from("AKIDEXAMPLE");
        let secret_access_key = crate::SecretString::from("secret");
        let credentials = Credentials {
            access_key_id: &access_key_id,
            secret_access_key: &secret_access_key,
        };
        let (base_url, requests) = serve("403 Forbidden", r#"{"error":"denied"}"#);
        let endpoint = format!("{base_url}/?Action=GetSeatInfoUsage&Version=2024-01-01");

        let error = execute(
            &Client::new(),
            credentials,
            Request {
                seat_id: "seat-1",
                project_name: None,
            },
            &endpoint,
            X_DATE,
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::FORBIDDEN));
        assert_eq!(error.raw_body(), Some(r#"{"error":"denied"}"#));
        requests.recv().expect("captured request");
    }

    #[test]
    fn validates_documented_percentages_and_milestones() {
        let usage = Usage {
            seat_id: "seat-1".into(),
            account_id: 42,
            project_name: "default".into(),
            user_id: "user-1".into(),
            user_name: "Ada".into(),
            monthly_subscribe_milestone: 1_000,
            monthly_reset_milestone: 2_000,
            short_term_usage: 0.0,
            weekly_usage: 101.0,
            monthly_usage: f64::NAN,
            short_term_reset_milestone: -1,
            weekly_reset_milestone: 2_000,
        };
        let windows: Vec<_> = usage.windows().collect();

        assert_eq!(windows[0].reset_at_ms(), Ok(None));
        assert_eq!(
            windows[1].used_percent(),
            Err(UsageWindowError::PercentageOutOfRange)
        );
        assert_eq!(windows[2].used_percent(), Err(UsageWindowError::NonFinite));
        assert_eq!(
            usage.monthly_period(),
            Ok(MillisecondPeriod {
                start_ms: 1_000,
                reset_ms: 2_000
            })
        );

        let invalid_reset = UsageWindow::new(WindowKind::Weekly, 0.0, -1);
        assert_eq!(
            invalid_reset.reset_at_ms(),
            Err(UsageWindowError::InvalidReset)
        );
    }
}
