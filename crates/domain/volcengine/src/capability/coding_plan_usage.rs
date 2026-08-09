//! Coding Plan Team seat quota usage.
//!
//! This wraps Volcengine's documented `GetSeatInfoUsage` action. The personal
//! Coding Plan currently exposes quota usage only in the console.
//! Contract: <https://www.volcengine.com/docs/82379/2306579>

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::{Credentials, signing};

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
    #[serde(rename = "SeatID")]
    pub seat_id: String,
    #[serde(rename = "AccountID")]
    pub account_id: u64,
    pub project_name: String,
    #[serde(rename = "UserID")]
    pub user_id: String,
    pub user_name: String,
    pub monthly_subscribe_milestone: i64,
    pub monthly_reset_milestone: i64,
    pub short_term_usage: f64,
    pub weekly_usage: f64,
    pub monthly_usage: f64,
    pub short_term_reset_milestone: i64,
    pub weekly_reset_milestone: i64,
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

pub async fn get(
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
    if credentials.access_key_id.trim().is_empty() {
        return Err(Error::InvalidCredentials("access_key_id"));
    }
    if credentials.secret_access_key.trim().is_empty() {
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
    async fn sends_signed_request_and_decodes_usage() {
        let response = r#"{
            "ResponseMetadata":{"RequestId":"request-1","Action":"GetSeatInfoUsage","Version":"2024-01-01","Service":"ark","Region":"cn-beijing"},
            "Result":{"SeatID":"seat-1","AccountID":42,"ProjectName":"default","UserID":"user-1","UserName":"Ada","MonthlySubscribeMilestone":1786320000000,"MonthlyResetMilestone":1788998400000,"ShortTermUsage":12.5,"WeeklyUsage":20.0,"MonthlyUsage":30.0,"ShortTermResetMilestone":1786341600000,"WeeklyResetMilestone":1786924800000}
        }"#;
        let (endpoint, requests) = serve("200 OK", response);

        let response = execute(
            &Client::new(),
            CREDENTIALS,
            Request {
                seat_id: "seat-1",
                project_name: Some("default"),
            },
            &endpoint,
            X_DATE,
        )
        .await
        .expect("request succeeds");

        let usage = response.result.expect("usage result");
        assert_eq!(usage.seat_id, "seat-1");
        assert_eq!(usage.account_id, 42);
        assert_eq!(usage.short_term_usage, 12.5);

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
        let (endpoint, requests) = serve("403 Forbidden", r#"{"error":"denied"}"#);

        let error = execute(
            &Client::new(),
            CREDENTIALS,
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
            format!("http://{address}/?Action=GetSeatInfoUsage&Version=2024-01-01"),
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
