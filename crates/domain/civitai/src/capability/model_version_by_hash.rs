//! Unauthenticated Civitai model-version lookup by a BLAKE3 file digest.
//!
//! Endpoint behavior follows the official
//! [Model Versions API](https://developer.civitai.com/site/reference/model-versions).

use std::{error, fmt, str::FromStr};

use reqwest::{Client, StatusCode, header};

use crate::model::ModelVersion;

const ENDPOINT: &str = "https://civitai.red/api/v1/model-versions/by-hash";
const USER_AGENT: &str = concat!("provider-civitai/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Blake3Hash([u8; 32]);

impl Blake3Hash {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for Blake3Hash {
    fn from(value: [u8; 32]) -> Self {
        Self::from_bytes(value)
    }
}

impl FromStr for Blake3Hash {
    type Err = ParseBlake3HashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 {
            return Err(ParseBlake3HashError::InvalidLength);
        }

        let mut bytes = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex_digit(pair[0]).ok_or(ParseBlake3HashError::InvalidCharacter)?;
            let low = decode_hex_digit(pair[1]).ok_or(ParseBlake3HashError::InvalidCharacter)?;
            bytes[index] = (high << 4) | low;
        }

        Ok(Self(bytes))
    }
}

impl fmt::Display for Blake3Hash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02X}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseBlake3HashError {
    InvalidLength,
    InvalidCharacter,
}

impl fmt::Display for ParseBlake3HashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => {
                formatter.write_str("BLAKE3 hash must contain 64 hexadecimal characters")
            }
            Self::InvalidCharacter => {
                formatter.write_str("BLAKE3 hash contains a non-hexadecimal character")
            }
        }
    }
}

impl error::Error for ParseBlake3HashError {}

#[derive(Debug)]
pub enum Error {
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
            Self::Exchange(_) | Self::BodyRead { .. } => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyRead { status, .. } => {
                write!(
                    formatter,
                    "Civitai response body read failed after HTTP {status}"
                )
            }
            Self::Exchange(_) => formatter.write_str("Civitai model-version hash lookup failed"),
            Self::Response { status, .. } => {
                write!(
                    formatter,
                    "Civitai model-version hash lookup returned HTTP {status}"
                )
            }
            Self::Decode { .. } => {
                formatter.write_str("Civitai model-version hash lookup returned invalid JSON")
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::BodyRead { source, .. } => Some(source),
            Self::Exchange(source) => Some(source),
            Self::Decode { source, .. } => Some(source),
            Self::Response { .. } => None,
        }
    }
}

pub async fn call(client: &Client, hash: Blake3Hash) -> Result<ModelVersion, Error> {
    fetch_at(client, hash, ENDPOINT).await
}

async fn fetch_at(
    client: &Client,
    hash: Blake3Hash,
    endpoint: &str,
) -> Result<ModelVersion, Error> {
    let response = client
        .get(format!("{endpoint}/{hash}"))
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

const fn decode_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use reqwest::{Client, StatusCode};

    use super::{Blake3Hash, Error, ParseBlake3HashError, fetch_at};
    use provider_test_support::serve;

    const LOWER_HASH: &str = "1a411d9b9cb3896e76157c2167fb808b992e29d38d6c10410d7e42fdb830d48f";
    const UPPER_HASH: &str = "1A411D9B9CB3896E76157C2167FB808B992E29D38D6C10410D7E42FDB830D48F";

    #[test]
    fn parses_and_formats_full_blake3_hashes() {
        let lower = LOWER_HASH.parse::<Blake3Hash>().expect("valid hash");
        let upper = UPPER_HASH.parse::<Blake3Hash>().expect("valid hash");

        assert_eq!(lower, upper);
        assert_eq!(lower.to_string(), UPPER_HASH);
        assert_eq!(Blake3Hash::from(lower.into_bytes()).as_bytes().len(), 32);
        assert_eq!(
            "A1".parse::<Blake3Hash>(),
            Err(ParseBlake3HashError::InvalidLength)
        );
        assert_eq!(
            format!("{}G", &UPPER_HASH[..63]).parse::<Blake3Hash>(),
            Err(ParseBlake3HashError::InvalidCharacter)
        );
    }

    #[tokio::test]
    async fn requests_hash_and_decodes_matching_model_file() {
        let body = format!(
            r#"{{"id":290640,"modelId":257749,"name":"v1.0","baseModel":"SDXL 1.0","files":[{{"id":283681,"name":"example.safetensors","type":"Model","hashes":{{"BLAKE3":"{UPPER_HASH}"}},"metadata":{{"format":"SafeTensor"}},"primary":true}}],"images":[],"air":"urn:air:sdxl:lora:civitai:257749@290640"}}"#
        );
        let (base_url, requests) = serve("200 OK", "application/json", body.as_bytes());
        let hash = LOWER_HASH.parse().expect("valid hash");

        let version = fetch_at(
            &Client::new(),
            hash,
            &format!("{base_url}/api/v1/model-versions/by-hash"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(version.id, 290640);
        assert_eq!(version.model_id, Some(257749));
        assert_eq!(version.files[0].id, 283681);
        assert_eq!(
            version.files[0].hashes.get("BLAKE3").map(String::as_str),
            Some(UPPER_HASH)
        );
        assert_eq!(
            version.extra.get("air").and_then(serde_json::Value::as_str),
            Some("urn:air:sdxl:lora:civitai:257749@290640")
        );

        let request = requests.recv().expect("captured request");
        assert!(request.starts_with(&format!(
            "GET /api/v1/model-versions/by-hash/{UPPER_HASH} HTTP/1.1\r\n"
        )));
        let headers = request
            .split_once("\r\n\r\n")
            .expect("HTTP request")
            .0
            .to_ascii_lowercase();
        assert!(headers.contains("\r\naccept: application/json\r\n"));
        assert!(headers.contains("\r\nuser-agent: provider-civitai/"));
        assert!(!headers.contains("\r\nauthorization:"));
    }

    #[tokio::test]
    async fn preserves_not_found_status_and_body() {
        let body = br#"{"error":"No model version found for hash"}"#;
        let (base_url, requests) = serve("404 Not Found", "application/json", body);
        let hash = LOWER_HASH.parse().expect("valid hash");

        let error = fetch_at(
            &Client::new(),
            hash,
            &format!("{base_url}/api/v1/model-versions/by-hash"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
        assert_eq!(
            error.raw_body(),
            Some(r#"{"error":"No model version found for hash"}"#)
        );
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn preserves_invalid_json_body() {
        let body = br#"{"id":"not-a-number"}"#;
        let (base_url, requests) = serve("200 OK", "application/json", body);
        let hash = LOWER_HASH.parse().expect("valid hash");

        let error = fetch_at(
            &Client::new(),
            hash,
            &format!("{base_url}/api/v1/model-versions/by-hash"),
        )
        .await
        .expect_err("decode fails");

        assert!(matches!(error, Error::Decode { .. }));
        assert_eq!(error.raw_body(), Some(r#"{"id":"not-a-number"}"#));
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn preserves_status_when_response_body_is_truncated() {
        for (status, expected) in [
            ("200 OK", StatusCode::OK),
            ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
        ] {
            let (endpoint, _requests) =
                provider_test_support::serve_truncated(status, "application/json");
            let error = fetch_at(&Client::new(), Blake3Hash::from_bytes([1; 32]), &endpoint)
                .await
                .expect_err("truncated response must fail");
            assert!(matches!(&error, Error::BodyRead { .. }));
            assert_eq!(error.status(), Some(expected));
            assert!(
                std::error::Error::source(&error)
                    .and_then(|source| source.downcast_ref::<reqwest::Error>())
                    .is_some()
            );
            assert_eq!(error.raw_body(), None);
        }
    }
}
