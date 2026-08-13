//! DeepSeek model discovery.
//!
//! Endpoint behavior follows the official
//! [Lists Models reference](https://api-docs.deepseek.com/api/list-models).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.deepseek.com/models";
const USER_AGENT: &str = concat!("provider-deepseek/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
pub struct Response {
    pub object: String,
    pub data: Vec<Model>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub owned_by: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => formatter.write_str("DeepSeek API key is empty"),
            Self::Exchange(_) => formatter.write_str("DeepSeek Models request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "DeepSeek Models returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("DeepSeek Models returned invalid JSON"),
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
    list_at(client, credentials, ENDPOINT).await
}

async fn list_at(
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
    use reqwest::StatusCode;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn lists_models() {
        let body = r#"{"object":"list","data":[{"id":"deepseek-chat","object":"model","owned_by":"deepseek","context_window":128000}]}"#;
        let (base_url, requests) = serve("200 OK", body);

        let response = list_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &format!("{base_url}/models"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.object, "list");
        assert_eq!(response.data[0].id, "deepseek-chat");
        assert_eq!(
            response.data[0].extra.get("context_window"),
            Some(&Value::from(128_000))
        );

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("get /models http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert!(request_body.is_empty());
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"invalid api key"}}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = list_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("bad-key")),
            &format!("{base_url}/models"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
