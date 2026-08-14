//! OpenCode Go quota usage.
//!
//! The public endpoint contract is defined by OpenCode's official
//! [server implementation](https://github.com/anomalyco/opencode/blob/4643e65ad6334de3e4e68dedc201d5fbb828c9fe/packages/console/app/src/routes/zen/go/v1/usage.ts).

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://opencode.ai/zen/go/v1/usage";
const USER_AGENT: &str = concat!("provider-opencode/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Response {
    pub usage: Usage,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Usage {
    pub rolling: UsageWindow,
    pub weekly: UsageWindow,
    pub monthly: UsageWindow,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub status: String,
    pub percent: f64,
    pub resets_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ProviderError {
    #[serde(rename = "type")]
    pub code: String,
    pub message: String,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ProviderError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageWindowError {
    NonFinite,
    PercentageOutOfRange,
}

impl UsageWindow {
    pub fn used_percent(&self) -> Result<f64, UsageWindowError> {
        if !self.percent.is_finite() {
            return Err(UsageWindowError::NonFinite);
        }
        if !(0.0..=100.0).contains(&self.percent) {
            return Err(UsageWindowError::PercentageOutOfRange);
        }
        Ok(self.percent)
    }

    pub fn remaining_percent(&self) -> Result<f64, UsageWindowError> {
        Ok(100.0 - self.used_percent()?)
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials,
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

    pub fn provider_error(&self) -> Option<ProviderError> {
        serde_json::from_str::<ErrorEnvelope>(self.raw_body()?)
            .ok()
            .map(|envelope| envelope.error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => formatter.write_str("OpenCode API key is empty"),
            Self::Exchange(_) => formatter.write_str("OpenCode Go usage request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "OpenCode Go usage returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("OpenCode Go usage returned invalid JSON"),
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

pub async fn call(client: &Client, credentials: Credentials<'_>) -> Result<Response, Error> {
    get_at(client, credentials, ENDPOINT).await
}

async fn get_at(
    client: &Client,
    credentials: Credentials<'_>,
    endpoint: &str,
) -> Result<Response, Error> {
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials);
    }

    let response = client
        .get(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
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

    #[tokio::test]
    async fn gets_go_usage() {
        let body = r#"{"usage":{"rolling":{"status":"ok","percent":12.5,"resetsAt":"2026-08-15T01:02:03.000Z"},"weekly":{"status":"ok","percent":25,"resetsAt":"2026-08-17T00:00:00.000Z"},"monthly":{"status":"rate-limited","percent":100,"resetsAt":"2026-09-01T00:00:00.000Z"}}}"#;
        let (base_url, requests) = serve("200 OK", body);

        let response = get_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("go-key")),
            &format!("{base_url}/zen/go/v1/usage"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.usage.rolling.percent, 12.5);
        assert_eq!(response.usage.rolling.remaining_percent(), Ok(87.5));
        assert_eq!(response.usage.monthly.status, "rate-limited");
        assert_eq!(response.usage.weekly.resets_at, "2026-08-17T00:00:00.000Z");

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("get /zen/go/v1/usage http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer go-key\r\n"));
        assert!(request_body.is_empty());
    }

    #[test]
    fn rejects_invalid_usage_percentages() {
        let window = UsageWindow {
            status: "ok".into(),
            percent: 101.0,
            resets_at: "2026-08-15T01:02:03.000Z".into(),
        };

        assert_eq!(
            window.remaining_percent(),
            Err(UsageWindowError::PercentageOutOfRange)
        );
    }

    #[test]
    fn decodes_typed_provider_error() {
        let error = Error::Response {
            status: StatusCode::FORBIDDEN,
            body: r#"{"type":"error","error":{"type":"EntitlementError","message":"OpenCode Go subscription required."},"debug":"not typed"}"#.into(),
        };

        assert_eq!(
            error.provider_error(),
            Some(ProviderError {
                code: "EntitlementError".into(),
                message: "OpenCode Go subscription required.".into(),
            })
        );
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

        (format!("http://{address}"), receiver)
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
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        request
    }
}
