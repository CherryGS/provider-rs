//! Models available to the caller's TypeSafe account.
//!
//! Wire contract: <https://docs.typesafe.ai/models>.

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/models";
const USER_AGENT: &str = concat!("provider-typesafe/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Response {
    pub models: Vec<Model>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Model {
    pub name: String,
    pub description: String,
    /// Provider-reported release date, retained without imposing a date format.
    pub release_date: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials,
    Exchange(reqwest::Error),
    /// Reading the response body failed after receiving the HTTP status.
    BodyRead {
        status: StatusCode,
        source: reqwest::Error,
    },
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
            Self::Response { status, .. } | Self::BodyRead { status, .. } => Some(*status),
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
            Self::InvalidCredentials => formatter.write_str("TypeSafe API key is empty"),
            Self::Exchange(_) => formatter.write_str("TypeSafe model list request failed"),
            Self::BodyRead { status, .. } => write!(
                formatter,
                "TypeSafe model list response body read failed after HTTP {status}"
            ),
            Self::Response { status, .. } => {
                write!(formatter, "TypeSafe model list returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("TypeSafe model list returned invalid JSON"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Exchange(source) | Self::BodyRead { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub async fn call(client: &Client, credentials: Credentials<'_>) -> Result<Response, Error> {
    fetch_at(client, credentials, ENDPOINT).await
}

async fn fetch_at(
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
    let body = response
        .bytes()
        .await
        .map_err(|source| Error::BodyRead { status, source })?;
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
mod tests;
