//! SiliconFlow text and vision embedding creation.
//!
//! Endpoint behavior follows the official [global text contract] and
//! [text-and-vision contract].
//!
//! [global text contract]: https://docs.siliconflow.com/en/api-reference/embeddings/create-embeddings
//! [text-and-vision contract]: https://docs.siliconflow.cn/cn/api-reference/embeddings/create-embeddings

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Credentials;

const ENDPOINT: &str = "https://api.siliconflow.com/v1/embeddings";
const USER_AGENT: &str = concat!("provider-siliconflow/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Request {
    Text(TextRequest),
    Vision(VisionRequest),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextRequest {
    pub model: String,
    pub input: TextInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
}

impl TextRequest {
    pub fn new(model: impl Into<String>, input: impl Into<TextInput>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            encoding_format: None,
            dimensions: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TextInput {
    Text(String),
    Texts(Vec<String>),
}

impl From<&str> for TextInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for TextInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VisionRequest {
    pub model: String,
    pub input: VisionInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncate: Option<Truncate>,
}

impl VisionRequest {
    pub fn new(model: impl Into<String>, input: VisionInput) -> Self {
        Self {
            model: model.into(),
            input,
            encoding_format: None,
            dimensions: None,
            user: None,
            truncate: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VisionInput {
    Text(String),
    Content(VisionContent),
    Multimodal(Vec<VisionItem>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VisionContent {
    Text { text: String },
    Image { image: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VisionItem {
    Text(String),
    Content(VisionContent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EncodingFormat {
    Float,
    Base64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Truncate {
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub object: String,
    pub model: String,
    pub data: Vec<Embedding>,
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
    #[serde(default)]
    pub completion_tokens: Option<u64>,
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
                write!(formatter, "SiliconFlow credential `{field}` is empty")
            }
            Self::InvalidRequest(field) => {
                write!(
                    formatter,
                    "SiliconFlow Embeddings field `{field}` is invalid"
                )
            }
            Self::Exchange(_) => formatter.write_str("SiliconFlow Embeddings request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "SiliconFlow Embeddings returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("SiliconFlow Embeddings returned invalid JSON")
            }
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

    let response = client
        .post(endpoint)
        .bearer_auth(credentials.api_key)
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

fn validate(credentials: Credentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.trim().is_empty() {
        return Err(Error::InvalidCredentials("api_key"));
    }

    let (model, input_is_invalid, dimensions, user) = match request {
        Request::Text(request) => (
            &request.model,
            match &request.input {
                TextInput::Text(value) => value.trim().is_empty(),
                TextInput::Texts(values) => {
                    values.is_empty()
                        || values.len() > 32
                        || values.iter().any(|value| value.trim().is_empty())
                }
            },
            request.dimensions,
            None,
        ),
        Request::Vision(request) => (
            &request.model,
            match &request.input {
                VisionInput::Text(value) => value.trim().is_empty(),
                VisionInput::Content(content) => content_is_empty(content),
                VisionInput::Multimodal(items) => {
                    items.is_empty()
                        || items.len() > 32
                        || items.iter().any(|item| match item {
                            VisionItem::Text(value) => value.trim().is_empty(),
                            VisionItem::Content(content) => content_is_empty(content),
                        })
                }
            },
            request.dimensions,
            request.user.as_deref(),
        ),
    };

    if model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if input_is_invalid {
        return Err(Error::InvalidRequest("input"));
    }
    if dimensions == Some(0) {
        return Err(Error::InvalidRequest("dimensions"));
    }
    if user.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::InvalidRequest("user"));
    }
    Ok(())
}

fn content_is_empty(content: &VisionContent) -> bool {
    match content {
        VisionContent::Text { text } => text.trim().is_empty(),
        VisionContent::Image { image } => image.trim().is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_text_input_and_decodes_vectors() {
        let response_body = r#"{"object":"list","model":"Qwen/Qwen3-Embedding-8B","data":[{"object":"embedding","embedding":[0.125,-0.5],"index":0}],"usage":{"prompt_tokens":4,"total_tokens":4,"completion_tokens":0}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = TextRequest::new(
            "Qwen/Qwen3-Embedding-8B",
            TextInput::Texts(vec!["one".to_owned(), "two".to_owned()]),
        );
        request.dimensions = Some(2);
        request.encoding_format = Some(EncodingFormat::Float);

        let response = create_at(
            &Client::new(),
            Credentials::new("test-key"),
            &Request::Text(request),
            &format!("{base_url}/v1/embeddings"),
        )
        .await
        .expect("request succeeds");

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
            r#"{"model":"Qwen/Qwen3-Embedding-8B","input":["one","two"],"encoding_format":"float","dimensions":2}"#
        );
    }

    #[tokio::test]
    async fn sends_explicit_multimodal_input() {
        let response_body = r#"{"object":"list","model":"Qwen/Qwen3-VL-Embedding-8B","data":[{"object":"embedding","embedding":"AQID","index":0}],"usage":{"prompt_tokens":10,"total_tokens":10}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = VisionRequest::new(
            "Qwen/Qwen3-VL-Embedding-8B",
            VisionInput::Multimodal(vec![
                VisionItem::Text("caption".to_owned()),
                VisionItem::Content(VisionContent::Image {
                    image: "https://example.com/image.jpg".to_owned(),
                }),
            ]),
        );
        request.encoding_format = Some(EncodingFormat::Base64);
        request.truncate = Some(Truncate::Right);

        let response = create_at(
            &Client::new(),
            Credentials::new("test-key"),
            &Request::Vision(request),
            &format!("{base_url}/v1/embeddings"),
        )
        .await
        .expect("request succeeds");

        assert!(matches!(&response.data[0].embedding, Vector::Base64(value) if value == "AQID"));
        let request = requests.recv().expect("captured request");
        let (_, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        assert_eq!(
            body,
            r#"{"model":"Qwen/Qwen3-VL-Embedding-8B","input":["caption",{"image":"https://example.com/image.jpg"}],"encoding_format":"base64","truncate":"right"}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"message":"bad input"}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = create_at(
            &Client::new(),
            Credentials::new("test-key"),
            &Request::Text(TextRequest::new("BAAI/bge-m3", "hello")),
            &format!("{base_url}/v1/embeddings"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
