//! Volcengine Ark multimodal embedding creation.
//!
//! Endpoint behavior follows the official Volcengine Go SDK [`embeddings.go`]
//! and its [multimodal embedding models] at pinned commit
//! `57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1`.
//!
//! [`embeddings.go`]: https://github.com/volcengine/volcengine-go-sdk/blob/57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1/service/arkruntime/embeddings.go
//! [multimodal embedding models]: https://github.com/volcengine/volcengine-go-sdk/blob/57ad072d4f4407a98fe5c85e4c1dfb30af59bfa1/service/arkruntime/model/multimodalembedding.go

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ArkCredentials, ExposeSecret};

const ENDPOINT: &str = "https://ark.cn-beijing.volces.com/api/v3/embeddings/multimodal";
const USER_AGENT: &str = concat!("provider-volcengine/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
    VideoUrl { video_url: VideoUrl },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_video_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_frame_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_frame_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_frames: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub input: Vec<Input>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl Request {
    pub fn new(model: impl Into<String>, input: Vec<Input>) -> Self {
        Self {
            input,
            model: model.into(),
            encoding_format: None,
            dimensions: None,
            instructions: None,
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
    pub model: String,
    pub created: i64,
    pub object: String,
    pub data: Embedding,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Embedding {
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub sparse_embedding: Option<Vec<SparseEntry>>,
    #[serde(default)]
    pub multi_embedding: Option<Value>,
    pub object: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct SparseEntry {
    pub index: u64,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
    pub prompt_tokens_details: PromptTokenDetails,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct PromptTokenDetails {
    pub text_tokens: u64,
    pub image_tokens: u64,
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
                write!(
                    formatter,
                    "Ark Multimodal Embeddings field `{field}` is invalid"
                )
            }
            Self::Exchange(_) => formatter.write_str("Ark Multimodal Embeddings request failed"),
            Self::Response { status, .. } => {
                write!(
                    formatter,
                    "Ark Multimodal Embeddings returned HTTP {status}"
                )
            }
            Self::Decode { .. } => {
                formatter.write_str("Ark Multimodal Embeddings returned invalid JSON")
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
    if request.input.is_empty() {
        return Err(Error::InvalidRequest("input"));
    }
    for input in &request.input {
        match input {
            Input::Text { text } if text.trim().is_empty() => {
                return Err(Error::InvalidRequest("input.text"));
            }
            Input::ImageUrl { image_url } if image_url.url.trim().is_empty() => {
                return Err(Error::InvalidRequest("input.image_url"));
            }
            Input::VideoUrl { video_url } => validate_video(video_url)?,
            _ => {}
        }
    }
    if request
        .dimensions
        .is_some_and(|value| !matches!(value, 1024 | 2048))
    {
        return Err(Error::InvalidRequest("dimensions"));
    }
    if request
        .instructions
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidRequest("instructions"));
    }
    Ok(())
}

fn validate_video(video: &VideoUrl) -> Result<(), Error> {
    if video.url.trim().is_empty() {
        return Err(Error::InvalidRequest("input.video_url"));
    }
    if video.fps.is_some_and(|value| !(0.2..=5.0).contains(&value)) {
        return Err(Error::InvalidRequest("input.video_url.fps"));
    }
    if video
        .max_video_tokens
        .is_some_and(|value| !(10_240..=204_800).contains(&value))
    {
        return Err(Error::InvalidRequest("input.video_url.max_video_tokens"));
    }
    if video
        .min_frame_tokens
        .is_some_and(|value| !(16..=128).contains(&value))
    {
        return Err(Error::InvalidRequest("input.video_url.min_frame_tokens"));
    }
    if video
        .max_frame_tokens
        .is_some_and(|value| !(128..=640).contains(&value))
    {
        return Err(Error::InvalidRequest("input.video_url.max_frame_tokens"));
    }
    if video
        .min_frames
        .is_some_and(|value| !(5..=16).contains(&value))
    {
        return Err(Error::InvalidRequest("input.video_url.min_frames"));
    }
    if video
        .min_frame_tokens
        .zip(video.max_frame_tokens)
        .is_some_and(|(min, max)| max < min)
    {
        return Err(Error::InvalidRequest("input.video_url.max_frame_tokens"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_mixed_input_and_decodes_multimodal_usage() {
        let response_body = r#"{"id":"embd-mm-1","model":"doubao-embedding-vision-251215","created":1786348800,"object":"list","data":{"embedding":[0.125,-0.5],"sparse_embedding":[{"index":7,"value":0.8}],"object":"embedding"},"usage":{"prompt_tokens":12,"total_tokens":12,"prompt_tokens_details":{"text_tokens":4,"image_tokens":8}}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new(
            "doubao-embedding-vision-251215",
            vec![
                Input::Text {
                    text: "red square".to_owned(),
                },
                Input::ImageUrl {
                    image_url: ImageUrl {
                        url: "https://example.com/red.png".to_owned(),
                    },
                },
            ],
        );
        request.encoding_format = Some(EncodingFormat::Float);
        request.dimensions = Some(2048);
        request.instructions = Some("Represent items for retrieval.".to_owned());

        let response = create_at(
            &Client::new(),
            ArkCredentials::new(&crate::SecretString::from("test-key")),
            &request,
            &format!("{base_url}/api/v3/embeddings/multimodal"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.data.embedding, [0.125, -0.5]);
        assert_eq!(response.usage.prompt_tokens_details.image_tokens, 8);

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /api/v3/embeddings/multimodal http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"input":[{"type":"text","text":"red square"},{"type":"image_url","image_url":{"url":"https://example.com/red.png"}}],"model":"doubao-embedding-vision-251215","encoding_format":"float","dimensions":2048,"instructions":"Represent items for retrieval."}"#
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
                "doubao-embedding-vision-251215",
                vec![Input::Text {
                    text: "hello".to_owned(),
                }],
            ),
            &format!("{base_url}/api/v3/embeddings/multimodal"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
