//! OpenAI embedding creation.
//!
//! Endpoint behavior follows `openai-node` v7.4.0
//! `src/resources/embeddings.ts` and the official
//! [Embeddings reference](https://platform.openai.com/docs/api-reference/embeddings/create).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.openai.com/v1/embeddings";
const USER_AGENT: &str = concat!("provider-openai/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Input {
    Text(String),
    Texts(Vec<String>),
    Tokens(Vec<u32>),
    TokenArrays(Vec<Vec<u32>>),
}

impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Input {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EncodingFormat {
    Float,
    Base64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub input: Input,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl Request {
    pub fn new(model: impl Into<String>, input: impl Into<Input>) -> Self {
        Self {
            input: input.into(),
            model: model.into(),
            dimensions: None,
            encoding_format: None,
            user: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub data: Vec<Embedding>,
    pub model: String,
    pub object: String,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Embedding {
    pub embedding: Vector,
    pub index: u64,
    pub object: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Vector {
    Float(Vec<f32>),
    Base64(String),
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
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
                write!(formatter, "OpenAI credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(formatter, "OpenAI Embeddings field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("OpenAI Embeddings request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "OpenAI Embeddings returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("OpenAI Embeddings returned invalid JSON"),
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
    create_at(client, credentials, request, ENDPOINT).await
}

async fn create_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let mut builder = client
        .post(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .json(request);
    if let Some(organization) = credentials.organization {
        builder = builder.header("OpenAI-Organization", organization.expose_secret());
    }
    if let Some(project) = credentials.project {
        builder = builder.header("OpenAI-Project", project.expose_secret());
    }

    let response = builder.send().await.map_err(Error::Exchange)?;
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

fn validate(credentials: Credentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("api_key"));
    }
    if credentials
        .organization
        .is_some_and(|value| value.expose_secret().trim().is_empty())
    {
        return Err(Error::InvalidCredentials("organization"));
    }
    if credentials
        .project
        .is_some_and(|value| value.expose_secret().trim().is_empty())
    {
        return Err(Error::InvalidCredentials("project"));
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    let empty_input = match &request.input {
        Input::Text(value) => value.is_empty(),
        Input::Texts(values) => values.is_empty() || values.iter().any(String::is_empty),
        Input::Tokens(values) => values.is_empty(),
        Input::TokenArrays(values) => values.is_empty() || values.iter().any(Vec::is_empty),
    };
    if empty_input {
        return Err(Error::InvalidRequest("input"));
    }
    if request.dimensions == Some(0) {
        return Err(Error::InvalidRequest("dimensions"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_input_and_decodes_vectors() {
        let response_body = r#"{"data":[{"embedding":[0.125,-0.5],"index":0,"object":"embedding"},{"embedding":[0.25,0.75],"index":1,"object":"embedding"}],"model":"text-embedding-3-small","object":"list","usage":{"prompt_tokens":4,"total_tokens":4}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "text-embedding-3-small",
            Input::Texts(vec!["one".to_owned(), "two".to_owned()]),
        );
        request.dimensions = Some(2);
        request.encoding_format = Some(EncodingFormat::Float);

        let response = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &request,
            &format!("{base_url}/v1/embeddings"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.data.len(), 2);
        assert!(
            matches!(&response.data[0].embedding, Vector::Float(values) if values == &[0.125, -0.5])
        );
        assert_eq!(response.usage.total_tokens, 4);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/embeddings http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"input":["one","two"],"model":"text-embedding-3-small","dimensions":2,"encoding_format":"float"}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"bad input"}}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::new("text-embedding-3-small", "hello"),
            &format!("{base_url}/v1/embeddings"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
