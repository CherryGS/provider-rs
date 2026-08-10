//! SiliconFlow model discovery.
//!
//! Endpoint behavior follows the official
//! [List models reference](https://docs.siliconflow.com/en/api-reference/models/get-model-list).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Credentials;

const ENDPOINT: &str = "https://api.siliconflow.com/v1/models";
const USER_AGENT: &str = concat!("provider-siliconflow/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Request {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub model_type: Option<ModelType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    Text,
    Image,
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubType {
    Chat,
    Embedding,
    Reranker,
    TextToImage,
    ImageToImage,
    SpeechToText,
    TextToVideo,
}

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
    #[serde(default)]
    pub created: Option<i64>,
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
            Self::InvalidCredentials => formatter.write_str("SiliconFlow API key is empty"),
            Self::Exchange(_) => formatter.write_str("SiliconFlow Models request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "SiliconFlow Models returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("SiliconFlow Models returned invalid JSON"),
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
    if credentials.api_key.trim().is_empty() {
        return Err(Error::InvalidCredentials);
    }

    let response = client
        .get(endpoint)
        .bearer_auth(credentials.api_key)
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

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn lists_filtered_models() {
        let body = r#"{"object":"list","data":[{"id":"stabilityai/test","object":"model","created":0,"owned_by":"stabilityai","available":true}]}"#;
        let (base_url, requests) = serve("200 OK", body);
        let request = Request {
            model_type: Some(ModelType::Image),
            sub_type: Some(SubType::TextToImage),
        };

        let response = list_at(
            &Client::new(),
            Credentials::new("test-key"),
            &request,
            &format!("{base_url}/v1/models"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.data[0].id, "stabilityai/test");
        assert_eq!(response.data[0].created, Some(0));
        assert_eq!(
            response.data[0].extra.get("available"),
            Some(&Value::Bool(true))
        );

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(
            headers.starts_with("get /v1/models?type=image&sub_type=text-to-image http/1.1\r\n")
        );
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert!(request_body.is_empty());
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"message":"invalid token"}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = list_at(
            &Client::new(),
            Credentials::new("bad-key"),
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
