//! OpenAI model discovery.
//!
//! Endpoint behavior follows `openai-node` v7.4.0
//! `src/resources/models.ts` and the official
//! [Models reference](https://developers.openai.com/api/reference/resources/models/methods/list).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.openai.com/v1/models";
const USER_AGENT: &str = concat!("provider-openai/", env!("CARGO_PKG_VERSION"));

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
    pub created: i64,
    pub object: String,
    pub owned_by: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials(&'static str),
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
            Self::Exchange(_) => formatter.write_str("OpenAI Models request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "OpenAI Models returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("OpenAI Models returned invalid JSON"),
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
    validate(credentials)?;

    let mut builder = client
        .get(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT);
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

fn validate(credentials: Credentials<'_>) -> Result<(), Error> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::*;
    use provider_test_support::serve_json as serve;

    #[tokio::test]
    async fn lists_models_with_scope_headers() {
        let body = r#"{"object":"list","data":[{"id":"gpt-test","created":1,"object":"model","owned_by":"openai","preview":true}],"next":null}"#;
        let (base_url, requests) = serve("200 OK", body);

        let response = list_at(
            &Client::new(),
            Credentials {
                api_key: &crate::SecretString::from("test-key"),
                organization: Some(&crate::SecretString::from("org-test")),
                project: Some(&crate::SecretString::from("proj-test")),
            },
            &format!("{base_url}/v1/models"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.object, "list");
        assert_eq!(response.data[0].id, "gpt-test");
        assert_eq!(
            response.data[0].extra.get("preview"),
            Some(&Value::Bool(true))
        );
        assert_eq!(response.extra.get("next"), Some(&Value::Null));

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("get /v1/models http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert!(headers.contains("\r\nopenai-organization: org-test\r\n"));
        assert!(headers.contains("\r\nopenai-project: proj-test\r\n"));
        assert!(request_body.is_empty());
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = r#"{"error":{"message":"unauthorized"}}"#;
        let (base_url, requests) = serve("401 Unauthorized", body);

        let error = list_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("bad-key")),
            &format!("{base_url}/v1/models"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::UNAUTHORIZED));
        assert_eq!(error.raw_body(), Some(body));
        requests.recv().expect("captured request");
    }
}
