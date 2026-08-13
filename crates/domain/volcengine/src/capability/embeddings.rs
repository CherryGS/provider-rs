//! Volcengine Ark text embedding creation.
//!
//! Endpoint behavior follows the official Volcengine Go SDK [`embeddings.go`]
//! and its [text embedding models] at pinned commit
//! `57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1`.
//!
//! [`embeddings.go`]: https://github.com/volcengine/volcengine-go-sdk/blob/57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1/service/arkruntime/embeddings.go
//! [text embedding models]: https://github.com/volcengine/volcengine-go-sdk/blob/57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1/service/arkruntime/model/embeddings.go

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ArkCredentials, ExposeSecret};

const ENDPOINT: &str = "https://ark.cn-beijing.volces.com/api/v3/embeddings";
const USER_AGENT: &str = concat!("provider-volcengine/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Input {
    Texts(Vec<String>),
    TokenArrays(Vec<Vec<u32>>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub input: Input,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
}

impl Request {
    pub fn new(model: impl Into<String>, input: Input) -> Self {
        Self {
            input,
            model: model.into(),
            user: None,
            encoding_format: None,
            dimensions: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EncodingFormat {
    Float,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub created: i64,
    pub object: String,
    pub data: Vec<Embedding>,
    pub model: String,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Embedding {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<Value>,
    #[serde(default)]
    pub completion_tokens_details: Option<Value>,
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
            Self::InvalidCredentials(field) => {
                write!(formatter, "Ark credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(formatter, "Ark Embeddings field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Ark Embeddings request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Ark Embeddings returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("Ark Embeddings returned invalid JSON"),
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
    credentials: ArkCredentials<'_>,
    request: &Request,
) -> Result<Response, Error> {
    create_at(client, credentials, request, ENDPOINT).await
}

async fn create_at(
    client: &Client,
    credentials: ArkCredentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let response = client
        .post(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .json(request)
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

fn validate(credentials: ArkCredentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("api_key"));
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    let input_is_invalid = match &request.input {
        Input::Texts(values) => {
            values.is_empty() || values.iter().any(|value| value.trim().is_empty())
        }
        Input::TokenArrays(values) => values.is_empty() || values.iter().any(Vec::is_empty),
    };
    if input_is_invalid {
        return Err(Error::InvalidRequest("input"));
    }
    if request
        .user
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("user"));
    }
    if request.dimensions == Some(0) {
        return Err(Error::InvalidRequest("dimensions"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_texts_and_decodes_vectors() {
        let response_body = r#"{"id":"embd-1","created":1786348800,"object":"list","data":[{"object":"embedding","embedding":[0.125,-0.5],"index":0},{"object":"embedding","embedding":[0.25,0.75],"index":1}],"model":"doubao-embedding-large-text","usage":{"prompt_tokens":4,"completion_tokens":0,"total_tokens":4}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "doubao-embedding-large-text",
            Input::Texts(vec!["one".to_owned(), "two".to_owned()]),
        );
        request.encoding_format = Some(EncodingFormat::Float);
        request.dimensions = Some(1024);

        let response = create_at(
            &Client::new(),
            ArkCredentials::new(&crate::SecretString::from("test-key")),
            &request,
            &format!("{base_url}/api/v3/embeddings"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.data[0].embedding, [0.125, -0.5]);
        assert_eq!(response.usage.total_tokens, 4);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /api/v3/embeddings http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"input":["one","two"],"model":"doubao-embedding-large-text","encoding_format":"float","dimensions":1024}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"code":"InvalidParameter","message":"bad input"}}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = create_at(
            &Client::new(),
            ArkCredentials::new(&crate::SecretString::from("test-key")),
            &Request::new(
                "doubao-embedding-large-text",
                Input::Texts(vec!["hello".to_owned()]),
            ),
            &format!("{base_url}/api/v3/embeddings"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
