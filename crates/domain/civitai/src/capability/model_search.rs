//! Unauthenticated Civitai model search over the selected `.red` endpoint.
//!
//! Query and response behavior follow the official
//! [Models API](https://developer.civitai.com/site/reference/models).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Model;

const ENDPOINT: &str = "https://civitai.red/api/v1/models";
const USER_AGENT: &str = concat!("provider-civitai/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_models: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_generation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_file_only: Option<bool>,
}

impl Request {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: Some(query.into()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub items: Vec<Model>,
    pub metadata: Metadata,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub next_page: Option<String>,
    #[serde(default)]
    pub current_page: Option<u64>,
    #[serde(default)]
    pub page_size: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
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
            Self::InvalidRequest(field) => {
                write!(formatter, "Civitai model search field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Civitai model search request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Civitai model search returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("Civitai model search returned invalid JSON")
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

pub async fn call(client: &Client, request: &Request) -> Result<Response, Error> {
    search_at(client, request, ENDPOINT).await
}

async fn search_at(client: &Client, request: &Request, endpoint: &str) -> Result<Response, Error> {
    validate(request)?;

    let response = client
        .get(endpoint)
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

fn validate(request: &Request) -> Result<(), Error> {
    if request
        .limit
        .is_some_and(|limit| !(1..=100).contains(&limit))
    {
        return Err(Error::InvalidRequest("limit"));
    }

    for (name, value) in [
        ("query", request.query.as_deref()),
        ("cursor", request.cursor.as_deref()),
        ("tag", request.tag.as_deref()),
        ("username", request.username.as_deref()),
        ("types", request.types.as_deref()),
        ("base_models", request.base_models.as_deref()),
        ("sort", request.sort.as_deref()),
        ("period", request.period.as_deref()),
    ] {
        if value.is_some_and(str::is_empty) {
            return Err(Error::InvalidRequest(name));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::{Client, StatusCode};

    use super::{Error, Request, search_at};
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_query_and_decodes_models() {
        let body = br#"{"items":[{"id":42,"name":"Pony Style","description":null,"type":"LORA","nsfw":false,"modelVersions":[{"id":84,"name":"v1","baseModel":"Pony","files":[],"images":[{"id":7,"url":"https://image.civitai.com/preview.jpeg","width":512,"height":768,"type":"image"}]}]}],"metadata":{"nextCursor":"84|42","nextPage":"https://civitai.red/api/v1/models?cursor=84%7C42"}}"#;
        let (base_url, requests) = serve("200 OK", "application/json", body);
        let mut request = Request::new("pony style");
        request.limit = Some(2);
        request.cursor = Some("84|42".to_owned());
        request.base_models = Some("Pony".to_owned());
        request.nsfw = Some(false);

        let response = search_at(
            &Client::new(),
            &request,
            &format!("{base_url}/api/v1/models"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.items[0].id, 42);
        assert_eq!(response.items[0].model_versions[0].images[0].id, Some(7));
        assert_eq!(response.metadata.next_cursor.as_deref(), Some("84|42"));

        let request = requests.recv().expect("captured request");
        let headers = request
            .split_once("\r\n\r\n")
            .expect("HTTP request")
            .0
            .to_ascii_lowercase();
        assert!(headers.starts_with(
            "get /api/v1/models?query=pony+style&limit=2&cursor=84%7c42&basemodels=pony&nsfw=false http/1.1\r\n"
        ));
        assert!(headers.contains("\r\naccept: application/json\r\n"));
        assert!(!headers.contains("\r\nauthorization:"));
    }

    #[tokio::test]
    async fn validates_requests_and_preserves_unsuccessful_responses() {
        let invalid = Request {
            limit: Some(0),
            ..Request::default()
        };
        assert!(matches!(
            search_at(&Client::new(), &invalid, "http://unused").await,
            Err(Error::InvalidRequest("limit"))
        ));

        let body = br#"{"error":"invalid cursor"}"#;
        let (base_url, requests) = serve("400 Bad Request", "application/json", body);
        let error = search_at(
            &Client::new(),
            &Request::default(),
            &format!("{base_url}/api/v1/models"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(error.raw_body(), Some(r#"{"error":"invalid cursor"}"#));
        requests.recv().expect("captured request");
    }
}
