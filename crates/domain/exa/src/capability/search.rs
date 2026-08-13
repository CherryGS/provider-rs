//! Exa native search.
//!
//! Contract follows Exa's official [Search reference](https://exa.ai/docs/reference/search).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.exa.ai/search";
const USER_AGENT: &str = concat!("provider-exa/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchType {
    Instant,
    Fast,
    Auto,
    DeepLite,
    Deep,
    DeepReasoning,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub query: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub search_type: Option<SearchType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_results: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_queries: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<String>,
}

impl Request {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            search_type: None,
            category: None,
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            start_published_date: None,
            end_published_date: None,
            moderation: None,
            contents: None,
            additional_queries: None,
            user_location: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    #[serde(default)]
    pub request_id: Option<String>,
    pub results: Vec<SearchResult>,
    #[serde(default)]
    pub cost_dollars: Option<Value>,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub favicon: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub highlight_scores: Vec<f64>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub subpages: Vec<Value>,
    #[serde(default)]
    pub entities: Vec<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials,
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
            Self::InvalidCredentials => formatter.write_str("Exa API key is empty"),
            Self::InvalidRequest(field) => {
                write!(formatter, "Exa Search field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("Exa Search request failed"),
            Self::Response { status, .. } => write!(formatter, "Exa Search returned HTTP {status}"),
            Self::Decode { .. } => formatter.write_str("Exa Search returned invalid JSON"),
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
    search_at(client, credentials, request, ENDPOINT).await
}

async fn search_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let response = client
        .post(endpoint)
        .header("x-api-key", credentials.api_key.expose_secret())
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
        return Err(Error::InvalidCredentials);
    }
    if request.query.trim().is_empty() {
        return Err(Error::InvalidRequest("query"));
    }
    if request
        .num_results
        .is_some_and(|num_results| !(1..=100).contains(&num_results))
    {
        return Err(Error::InvalidRequest("num_results"));
    }
    if request
        .include_domains
        .as_ref()
        .is_some_and(|domains| domains.len() > 1200)
    {
        return Err(Error::InvalidRequest("include_domains"));
    }
    if request
        .exclude_domains
        .as_ref()
        .is_some_and(|domains| domains.len() > 1200)
    {
        return Err(Error::InvalidRequest("exclude_domains"));
    }
    if request
        .additional_queries
        .as_ref()
        .is_some_and(|queries| queries.is_empty() || queries.len() > 10)
    {
        return Err(Error::InvalidRequest("additional_queries"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn sends_search_shape_and_decodes_response() {
        let api_key = crate::SecretString::from("test-key");
        let credentials = Credentials { api_key: &api_key };
        let response_body = r#"{"requestId":"req_1","results":[{"id":"result_1","title":"Exa","url":"https://exa.ai","text":"Search API","newField":true}],"costDollars":{"total":0.005},"future":true}"#;
        let (base_url, requests) = serve("200 OK", response_body);
        let mut request = Request::new("Rust provider libraries");
        request.search_type = Some(SearchType::DeepLite);
        request.category = Some("publication".to_owned());
        request.num_results = Some(3);
        request.include_domains = Some(vec!["exa.ai".to_owned()]);
        request.moderation = Some(true);
        request.contents = Some(json!({"text": {"maxCharacters": 1000}}));
        request.additional_queries = Some(vec!["Rust API clients".to_owned()]);
        request.user_location = Some("US".to_owned());

        let response = search_at(
            &Client::new(),
            credentials,
            &request,
            &format!("{base_url}/search"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.request_id.as_deref(), Some("req_1"));
        assert_eq!(response.results[0].url.as_deref(), Some("https://exa.ai"));
        assert_eq!(
            response.results[0].extra.get("newField"),
            Some(&json!(true))
        );
        assert_eq!(response.cost_dollars, Some(json!({"total": 0.005})));
        assert_eq!(response.extra.get("future"), Some(&json!(true)));

        let request = requests.recv().expect("captured request");
        let (headers, body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("post /search http/1.1\r\n"));
        assert!(headers.contains("\r\nx-api-key: test-key\r\n"));
        assert_eq!(
            body,
            r#"{"query":"Rust provider libraries","type":"deep-lite","category":"publication","numResults":3,"includeDomains":["exa.ai"],"moderation":true,"contents":{"text":{"maxCharacters":1000}},"additionalQueries":["Rust API clients"],"userLocation":"US"}"#
        );
    }

    #[tokio::test]
    async fn rejects_invalid_request_before_exchange() {
        let api_key = crate::SecretString::from("test-key");
        let credentials = Credentials { api_key: &api_key };
        let mut request = Request::new(" ");
        assert!(matches!(
            search_at(&Client::new(), credentials, &request, "invalid").await,
            Err(Error::InvalidRequest("query"))
        ));

        request.query = "valid".to_owned();
        request.num_results = Some(0);
        assert!(matches!(
            search_at(&Client::new(), credentials, &request, "invalid").await,
            Err(Error::InvalidRequest("num_results"))
        ));
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let api_key = crate::SecretString::from("test-key");
        let credentials = Credentials { api_key: &api_key };
        let body = r#"{"error":"rate limited"}"#;
        let (base_url, requests) = serve("429 Too Many Requests", body);

        let error = search_at(
            &Client::new(),
            credentials,
            &Request::new("Rust"),
            &format!("{base_url}/search"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
