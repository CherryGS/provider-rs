//! SiliconFlow text and vision reranking.
//!
//! Endpoint behavior follows the official [global text contract] and
//! [text-and-vision contract].
//!
//! [global text contract]: https://docs.siliconflow.com/en/api-reference/rerank/create-rerank
//! [text-and-vision contract]: https://docs.siliconflow.cn/cn/api-reference/rerank/create-rerank

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.siliconflow.com/v1/rerank";
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
    pub query: String,
    pub documents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_documents: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_chunks_per_doc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_tokens: Option<u32>,
}

impl TextRequest {
    pub fn new(model: impl Into<String>, query: impl Into<String>, documents: Vec<String>) -> Self {
        Self {
            model: model.into(),
            query: query.into(),
            documents,
            instruction: None,
            top_n: None,
            return_documents: None,
            max_chunks_per_doc: None,
            overlap_tokens: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VisionRequest {
    pub model: String,
    pub query: VisionQuery,
    pub documents: Vec<VisionDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_documents: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_chunks_per_doc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_tokens: Option<u32>,
}

impl VisionRequest {
    pub fn new(
        model: impl Into<String>,
        query: VisionQuery,
        documents: Vec<VisionDocument>,
    ) -> Self {
        Self {
            model: model.into(),
            query,
            documents,
            instruction: None,
            top_n: None,
            return_documents: None,
            max_chunks_per_doc: None,
            overlap_tokens: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VisionQuery {
    Text(String),
    Image { image: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VisionDocument {
    Text(String),
    TextContent { text: String },
    Image { image: String },
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub id: String,
    pub results: Vec<ResultItem>,
    #[serde(default)]
    pub meta: Option<Meta>,
    #[serde(default)]
    pub tokens: Option<TokenUsage>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct ResultItem {
    pub index: u64,
    pub relevance_score: f64,
    #[serde(default)]
    pub document: Option<Document>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Document {
    Text { text: String },
    Image { image: String },
}

#[derive(Debug, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub tokens: Option<TokenUsage>,
    #[serde(default)]
    pub billed_units: Option<BilledUnits>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub image_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct BilledUnits {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub image_tokens: Option<u64>,
    #[serde(default)]
    pub search_units: Option<u64>,
    #[serde(default)]
    pub classifications: Option<u64>,
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
                write!(formatter, "SiliconFlow Rerank field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("SiliconFlow Rerank request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "SiliconFlow Rerank returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("SiliconFlow Rerank returned invalid JSON"),
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

fn validate(credentials: Credentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials("api_key"));
    }

    let (model, query_is_invalid, documents_are_invalid, instruction, options) = match request {
        Request::Text(request) => (
            &request.model,
            request.query.trim().is_empty(),
            request.documents.is_empty()
                || request
                    .documents
                    .iter()
                    .any(|document| document.trim().is_empty()),
            request.instruction.as_deref(),
            (
                request.top_n,
                request.max_chunks_per_doc,
                request.overlap_tokens,
            ),
        ),
        Request::Vision(request) => (
            &request.model,
            match &request.query {
                VisionQuery::Text(value) => value.trim().is_empty(),
                VisionQuery::Image { image } => image.trim().is_empty(),
            },
            request.documents.is_empty()
                || request.documents.iter().any(|document| match document {
                    VisionDocument::Text(value) => value.trim().is_empty(),
                    VisionDocument::TextContent { text } => text.trim().is_empty(),
                    VisionDocument::Image { image } => image.trim().is_empty(),
                }),
            request.instruction.as_deref(),
            (
                request.top_n,
                request.max_chunks_per_doc,
                request.overlap_tokens,
            ),
        ),
    };

    if model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if query_is_invalid {
        return Err(Error::InvalidRequest("query"));
    }
    if documents_are_invalid {
        return Err(Error::InvalidRequest("documents"));
    }
    if instruction.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::InvalidRequest("instruction"));
    }
    if options.0 == Some(0) {
        return Err(Error::InvalidRequest("top_n"));
    }
    if options.1 == Some(0) {
        return Err(Error::InvalidRequest("max_chunks_per_doc"));
    }
    if options.2.is_some_and(|value| value > 80) {
        return Err(Error::InvalidRequest("overlap_tokens"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_text_documents_and_decodes_legacy_usage() {
        let response_body = r#"{"id":"rerank-1","results":[{"document":{"text":"apple"},"index":0,"relevance_score":0.95}],"tokens":{"input_tokens":5,"output_tokens":1}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = TextRequest::new(
            "Qwen/Qwen3-Reranker-8B",
            "Apple",
            vec!["apple".to_owned(), "banana".to_owned()],
        );
        request.top_n = Some(1);
        request.return_documents = Some(true);

        let response = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::Text(request),
            &format!("{base_url}/v1/rerank"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.results[0].index, 0);
        assert_eq!(
            response
                .tokens
                .as_ref()
                .and_then(|tokens| tokens.input_tokens),
            Some(5)
        );

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/rerank http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert_eq!(
            body,
            r#"{"model":"Qwen/Qwen3-Reranker-8B","query":"Apple","documents":["apple","banana"],"top_n":1,"return_documents":true}"#
        );
    }

    #[tokio::test]
    async fn sends_explicit_vision_documents() {
        let response_body = r#"{"id":"rerank-2","results":[{"document":{"image":"redacted"},"index":1,"relevance_score":0.85}],"meta":{"tokens":{"input_tokens":150,"image_tokens":42},"billed_units":{"input_tokens":150,"image_tokens":42}}}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let request = VisionRequest::new(
            "Qwen/Qwen3-VL-Reranker-8B",
            VisionQuery::Image {
                image: "https://example.com/query.jpg".to_owned(),
            },
            vec![
                VisionDocument::Text("caption".to_owned()),
                VisionDocument::Image {
                    image: "https://example.com/document.jpg".to_owned(),
                },
            ],
        );

        let response = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::Vision(request),
            &format!("{base_url}/v1/rerank"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(
            response
                .meta
                .as_ref()
                .and_then(|meta| meta.tokens.as_ref())
                .and_then(|tokens| tokens.image_tokens),
            Some(42)
        );
        let request = requests.recv().expect("captured request");
        let (_, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        assert_eq!(
            body,
            r#"{"model":"Qwen/Qwen3-VL-Reranker-8B","query":{"image":"https://example.com/query.jpg"},"documents":["caption",{"image":"https://example.com/document.jpg"}]}"#
        );
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"message":"bad input"}"#;
        let (base_url, requests) = serve("400 Bad Request", body);

        let error = create_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &Request::Text(TextRequest::new(
                "BAAI/bge-reranker-v2-m3",
                "Apple",
                vec!["apple".to_owned()],
            )),
            &format!("{base_url}/v1/rerank"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
