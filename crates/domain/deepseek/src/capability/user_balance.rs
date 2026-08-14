//! DeepSeek user balance.
//!
//! Endpoint behavior follows the official
//! [Get User Balance reference](https://api-docs.deepseek.com/api/get-user-balance/).

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.deepseek.com/user/balance";
const USER_AGENT: &str = concat!("provider-deepseek/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Response {
    pub is_available: bool,
    pub balance_infos: Vec<BalanceInfo>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
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
            Self::InvalidCredentials => formatter.write_str("DeepSeek API key is empty"),
            Self::Exchange(_) => formatter.write_str("DeepSeek User Balance request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "DeepSeek User Balance returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("DeepSeek User Balance returned invalid JSON")
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

pub async fn call(client: &Client, credentials: Credentials<'_>) -> Result<Response, Error> {
    get_at(client, credentials, ENDPOINT).await
}

async fn get_at(
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
    use super::*;
    use crate::test_support::serve;

    #[tokio::test]
    async fn gets_user_balance() {
        let body = r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"110.00","granted_balance":"10.00","topped_up_balance":"100.00","expires_at":null}]}"#;
        let (base_url, requests) = serve("200 OK", body);

        let response = get_at(
            &Client::new(),
            Credentials::new(&crate::SecretString::from("test-key")),
            &format!("{base_url}/user/balance"),
        )
        .await
        .expect("request succeeds");

        assert!(response.is_available);
        assert_eq!(response.balance_infos[0].currency, "CNY");
        assert_eq!(response.balance_infos[0].total_balance, "110.00");
        assert_eq!(
            response.balance_infos[0].extra.get("expires_at"),
            Some(&Value::Null)
        );

        let request = requests.recv().expect("captured request");
        let (headers, request_body) = request.split_once("\r\n\r\n").expect("HTTP request");
        let headers = headers.to_ascii_lowercase();
        assert!(headers.starts_with("get /user/balance http/1.1\r\n"));
        assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
        assert!(request_body.is_empty());
    }
}
