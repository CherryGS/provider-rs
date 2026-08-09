//! Agent Plan personal quota usage.
//!
//! This wraps Volcengine's documented `GetAFPUsage` action for the five-hour,
//! daily, weekly, and monthly Agent Plan quota windows.
//! Contract: <https://www.volcengine.com/docs/82379/2479847>

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::Deserialize;

use crate::{Credentials, signing};

const ACTION: &str = "GetAFPUsage";
const ENDPOINT: &str = "https://ark.cn-beijing.volces.com/?Action=GetAFPUsage&Version=2024-01-01";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Response {
    pub response_metadata: ResponseMetadata,
    pub result: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResponseMetadata {
    pub request_id: Option<String>,
    pub action: Option<String>,
    pub version: Option<String>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub error: Option<ProviderError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProviderError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct QuotaWindow {
    pub quota: f64,
    pub used: f64,
    pub subscribe_time: i64,
    pub reset_time: i64,
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

pub async fn get(client: &Client, credentials: Credentials<'_>) -> Result<Response, Error> {
    let x_date = signing::x_date().map_err(Error::Clock)?;
    execute(client, credentials, ENDPOINT, &x_date).await
}

async fn execute(
    client: &Client,
    credentials: Credentials<'_>,
    endpoint: &str,
    x_date: &str,
) -> Result<Response, Error> {
    if credentials.access_key_id.trim().is_empty() {
        return Err(Error::InvalidCredentials("access_key_id"));
    }
    if credentials.secret_access_key.trim().is_empty() {
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
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc::{self, Receiver},
        thread,
        time::Duration,
    };

    use super::*;

    const CREDENTIALS: Credentials<'static> = Credentials {
        access_key_id: "AKIDEXAMPLE",
        secret_access_key: "secret",
    };
    const X_DATE: &str = "20260810T120000Z";

    #[tokio::test]
    async fn sends_signed_request_and_decodes_quota_windows() {
        let response = r#"{
            "ResponseMetadata":{"RequestId":"request-1","Action":"GetAFPUsage","Version":"2024-01-01","Service":"ark","Region":"cn-beijing"},
            "Result":{"PlanType":"Large","AFPFiveHour":{"Quota":1000,"Used":125.5,"SubscribeTime":1786320000000,"ResetTime":1786341600000},"AFPDaily":{"Quota":5000,"Used":500,"SubscribeTime":1786320000000,"ResetTime":1786406400000},"AFPWeekly":{"Quota":20000,"Used":2000,"SubscribeTime":1786320000000,"ResetTime":1786924800000},"AFPMonthly":{"Quota":80000,"Used":8000,"SubscribeTime":1786320000000,"ResetTime":1788998400000}}
        }"#;
        let (endpoint, requests) = serve("200 OK", response);

        let response = execute(&Client::new(), CREDENTIALS, &endpoint, X_DATE)
            .await
            .expect("request succeeds");

        let usage = response.result.expect("usage result");
        assert_eq!(usage.plan_type, "Large");
        assert_eq!(usage.five_hour.used, 125.5);
        assert_eq!(usage.monthly.quota, 80_000.0);

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
        let (endpoint, requests) = serve("429 Too Many Requests", r#"{"error":"limited"}"#);

        let error = execute(&Client::new(), CREDENTIALS, &endpoint, X_DATE)
            .await
            .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(error.raw_body(), Some(r#"{"error":"limited"}"#));
        requests.recv().expect("captured request");
    }

    fn serve(status: &'static str, response_body: &str) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let response_body = response_body.to_owned();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");
            let request = read_request(&mut stream);
            sender
                .send(String::from_utf8(request).expect("UTF-8 request"))
                .expect("send captured request");
            write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response_body}",
                response_body.len()
            )
            .expect("write response");
        });

        (
            format!("http://{address}/?Action=GetAFPUsage&Version=2024-01-01"),
            receiver,
        )
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];

        loop {
            let read = stream.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);

            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&request[..header_end]).expect("UTF-8 headers");
            let content_length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }

        request
    }
}
