//! Agent Plan personal quota usage.
//!
//! This wraps Volcengine's documented `GetAFPUsage` action for the five-hour,
//! daily, weekly, and monthly Agent Plan quota windows.
//! Contract: <https://www.volcengine.com/docs/82379/2479847>

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};

use super::{EndpointOutcome, ErrorKind, MillisecondPeriod};
pub use super::{ProviderError, ResponseMetadata};
use crate::{Credentials, ExposeSecret, signing};

const ACTION: &str = "GetAFPUsage";
const ENDPOINT: &str = "https://ark.cn-beijing.volces.com/?Action=GetAFPUsage&Version=2024-01-01";

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
    pub plan_type: String,
    #[serde(rename = "AFPFiveHour")]
    pub five_hour: QuotaWindow,
    #[serde(rename = "AFPDaily")]
    pub daily: QuotaWindow,
    #[serde(rename = "AFPWeekly")]
    pub weekly: QuotaWindow,
    #[serde(rename = "AFPMonthly")]
    pub monthly: QuotaWindow,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuotaWindow {
    pub quota: f64,
    pub used: f64,
    pub subscribe_time: i64,
    pub reset_time: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowKind {
    FiveHour,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Clone, Copy, Debug)]
pub struct WindowRef<'a> {
    pub kind: WindowKind,
    pub window: &'a QuotaWindow,
}

impl Usage {
    pub fn windows(&self) -> impl ExactSizeIterator<Item = WindowRef<'_>> {
        [
            WindowRef {
                kind: WindowKind::FiveHour,
                window: &self.five_hour,
            },
            WindowRef {
                kind: WindowKind::Daily,
                window: &self.daily,
            },
            WindowRef {
                kind: WindowKind::Weekly,
                window: &self.weekly,
            },
            WindowRef {
                kind: WindowKind::Monthly,
                window: &self.monthly,
            },
        ]
        .into_iter()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaWindowError {
    NonFinite,
    Negative,
    ZeroQuota,
    UsageExceedsQuota,
    InvalidPeriod,
}

impl QuotaWindow {
    /// Computes used percentage from the documented period quota and usage.
    ///
    /// `Quota` is the period total and `Used` is the amount consumed according
    /// to `GetAFPUsage`, updated 2026-05-20:
    /// <https://www.volcengine.com/docs/82379/2479847>
    pub fn used_percent(&self) -> Result<f64, QuotaWindowError> {
        self.validate_amounts()?;
        Ok(self.used / self.quota * 100.0)
    }

    pub fn remaining_percent(&self) -> Result<f64, QuotaWindowError> {
        Ok(100.0 - self.used_percent()?)
    }

    /// Returns the provider timestamps as a validated Unix-millisecond period.
    ///
    /// Timestamp units and field meaning come from the `GetAFPUsage` contract:
    /// <https://www.volcengine.com/docs/82379/2479847>
    pub fn period(&self) -> Result<MillisecondPeriod, QuotaWindowError> {
        if self.subscribe_time <= 0 || self.reset_time <= self.subscribe_time {
            return Err(QuotaWindowError::InvalidPeriod);
        }
        Ok(MillisecondPeriod {
            start_ms: self.subscribe_time,
            reset_ms: self.reset_time,
        })
    }

    pub const fn subscribe_time_ms(&self) -> i64 {
        self.subscribe_time
    }

    pub const fn reset_time_ms(&self) -> i64 {
        self.reset_time
    }

    fn validate_amounts(&self) -> Result<(), QuotaWindowError> {
        if !self.quota.is_finite() || !self.used.is_finite() {
            return Err(QuotaWindowError::NonFinite);
        }
        if self.quota < 0.0 || self.used < 0.0 {
            return Err(QuotaWindowError::Negative);
        }
        if self.quota == 0.0 {
            return Err(QuotaWindowError::ZeroQuota);
        }
        if self.used > self.quota {
            return Err(QuotaWindowError::UsageExceedsQuota);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
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
            Self::Clock(_) => formatter.write_str("failed to format the Volcengine signing time"),
            Self::Signing => formatter.write_str("failed to sign the Agent Plan request"),
            Self::Exchange(_) => formatter.write_str("Agent Plan request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Agent Plan request returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("Agent Plan response was not valid JSON"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Clock(source) => Some(source),
            Self::Exchange(source) => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub async fn call(client: &Client, credentials: Credentials<'_>) -> Result<Response, Error> {
    let x_date = signing::x_date().map_err(Error::Clock)?;
    execute(client, credentials, ENDPOINT, &x_date).await
}

async fn execute(
    client: &Client,
    credentials: Credentials<'_>,
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

    let body = b"{}";
    let signed = signing::sign(ACTION, body, credentials, x_date).ok_or(Error::Signing)?;
    let response = client
        .post(endpoint)
        .header(header::HOST, signing::HOST)
        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
        .header("X-Date", x_date)
        .header("X-Content-Sha256", signed.payload_hash)
        .header(header::AUTHORIZATION, signed.authorization)
        .body(body.as_slice())
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
    async fn sends_signed_request_and_decodes_quota_windows() {
        let access_key_id = crate::SecretString::from("AKIDEXAMPLE");
        let secret_access_key = crate::SecretString::from("secret");
        let credentials = Credentials {
            access_key_id: &access_key_id,
            secret_access_key: &secret_access_key,
        };
        let response = r#"{
            "ResponseMetadata":{"RequestId":"request-1","Action":"GetAFPUsage","Version":"2024-01-01","Service":"ark","Region":"cn-beijing"},
            "Result":{"PlanType":"Large","AFPFiveHour":{"Quota":1000,"Used":125.5,"SubscribeTime":1786320000000,"ResetTime":1786341600000},"AFPDaily":{"Quota":5000,"Used":500,"SubscribeTime":1786320000000,"ResetTime":1786406400000},"AFPWeekly":{"Quota":20000,"Used":2000,"SubscribeTime":1786320000000,"ResetTime":1786924800000},"AFPMonthly":{"Quota":80000,"Used":8000,"SubscribeTime":1786320000000,"ResetTime":1788998400000}}
        }"#;
        let (base_url, requests) = serve("200 OK", response);
        let endpoint = format!("{base_url}/?Action=GetAFPUsage&Version=2024-01-01");

        let response = execute(&Client::new(), credentials, &endpoint, X_DATE)
            .await
            .expect("request succeeds");

        let usage = match response.into_outcome() {
            EndpointOutcome::Success { data, .. } => data,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        assert_eq!(usage.plan_type, "Large");
        assert_eq!(usage.five_hour.used, 125.5);
        assert_eq!(usage.monthly.quota, 80_000.0);
        assert_eq!(usage.five_hour.used_percent(), Ok(12.55));
        assert_eq!(usage.five_hour.remaining_percent(), Ok(87.45));
        assert_eq!(usage.windows().len(), 4);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /?action=getafpusage&version=2024-01-01 http/1.1\r\n"));
        assert!(headers.contains("\r\nhost: ark.cn-beijing.volces.com\r\n"));
        assert!(headers.contains(
            "\r\nx-content-sha256: 44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a\r\n"
        ));
        assert!(headers.contains(
            "\r\nauthorization: hmac-sha256 credential=akidexample/20260810/cn-beijing/ark/request, signedheaders=host;x-content-sha256;x-date, signature=0434a30bae92139ca2f0f9f4e460d33d82dec965c024a9b13bb49386f9ba9eb3\r\n"
        ));
        assert_eq!(body, "{}");
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let access_key_id = crate::SecretString::from("AKIDEXAMPLE");
        let secret_access_key = crate::SecretString::from("secret");
        let credentials = Credentials {
            access_key_id: &access_key_id,
            secret_access_key: &secret_access_key,
        };
        let (base_url, requests) = serve("429 Too Many Requests", r#"{"error":"limited"}"#);
        let endpoint = format!("{base_url}/?Action=GetAFPUsage&Version=2024-01-01");

        let error = execute(&Client::new(), credentials, &endpoint, X_DATE)
            .await
            .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(error.raw_body(), Some(r#"{"error":"limited"}"#));
        requests.recv().expect("captured request");
    }

    #[test]
    fn rejects_invalid_quota_values_and_periods() {
        let window = |quota, used| QuotaWindow {
            quota,
            used,
            subscribe_time: 1_000,
            reset_time: 2_000,
        };

        assert_eq!(
            window(0.0, 0.0).used_percent(),
            Err(QuotaWindowError::ZeroQuota)
        );
        assert_eq!(
            window(10.0, 11.0).remaining_percent(),
            Err(QuotaWindowError::UsageExceedsQuota)
        );
        assert_eq!(
            window(f64::INFINITY, 1.0).used_percent(),
            Err(QuotaWindowError::NonFinite)
        );
        assert_eq!(
            window(10.0, -1.0).used_percent(),
            Err(QuotaWindowError::Negative)
        );
        assert_eq!(
            QuotaWindow {
                reset_time: 1_000,
                ..window(10.0, 1.0)
            }
            .period(),
            Err(QuotaWindowError::InvalidPeriod)
        );
    }
}
