//! Retrieval of preview images discovered in Civitai model responses.
//!
//! Preview fields follow the official [Model Versions API]. The allowed
//! production image origin follows Civitai's [pinned host configuration].
//!
//! [Model Versions API]: https://developer.civitai.com/site/reference/model-versions
//! [pinned host configuration]: https://github.com/civitai/civitai/blob/2a2fe66428330a13c84fc2d7f8fb07d3cc61cf23/next.config.mjs

use std::{error, fmt};

use bytes::Bytes;
use reqwest::{Client, StatusCode, Url, header};

use crate::model::PreviewImage;

const USER_AGENT: &str = concat!("provider-civitai/", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub struct Response {
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Debug)]
pub enum Error {
    InvalidPreviewUrl,
    UnexpectedContentType,
    Exchange(reqwest::Error),
    Response { status: StatusCode, body: String },
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
            Self::Response { body, .. } => Some(body),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPreviewUrl => {
                formatter.write_str("Civitai preview URL is not an official HTTPS media URL")
            }
            Self::UnexpectedContentType => {
                formatter.write_str("Civitai preview response is not an image")
            }
            Self::Exchange(_) => formatter.write_str("Civitai preview image request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Civitai preview image returned HTTP {status}")
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Exchange(source) => Some(source),
            _ => None,
        }
    }
}

pub async fn call(client: &Client, preview: &PreviewImage) -> Result<Response, Error> {
    let url = validate_url(&preview.url)?;
    fetch_from(client, url).await
}

async fn fetch_from(client: &Client, url: Url) -> Result<Response, Error> {
    let response = client
        .get(url)
        .header(header::ACCEPT, "image/*")
        .header(header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(Error::Exchange)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.bytes().await.map_err(Error::Exchange)?;
        return Err(Error::Response {
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            value
                .as_bytes()
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"image/"))
        })
        .ok_or(Error::UnexpectedContentType)?
        .to_owned();
    let bytes = response.bytes().await.map_err(Error::Exchange)?;

    Ok(Response {
        content_type,
        bytes,
    })
}

fn validate_url(value: &str) -> Result<Url, Error> {
    let url = Url::parse(value).map_err(|_| Error::InvalidPreviewUrl)?;
    if url.scheme() != "https"
        || url.host_str() != Some("image.civitai.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidPreviewUrl);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use reqwest::{Client, StatusCode, Url};

    use super::{Error, call, fetch_from};
    use crate::{model::PreviewImage, test_support::serve};

    fn preview(url: impl Into<String>) -> PreviewImage {
        PreviewImage {
            id: Some(7),
            url: url.into(),
            nsfw_level: Some(1),
            width: Some(512),
            height: Some(768),
            hash: None,
            kind: Some("image".to_owned()),
            extra: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn requests_discovered_image_and_returns_binary_body() {
        let body = [0x52, 0x49, 0x46, 0x46];
        let (base_url, requests) = serve("200 OK", "image/webp", &body);

        let response = fetch_from(
            &Client::new(),
            Url::parse(&format!("{base_url}/preview.jpeg")).expect("valid test URL"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.content_type, "image/webp");
        assert_eq!(response.bytes.as_ref(), body);

        let request = requests.recv().expect("captured request");
        let headers = request
            .split_once("\r\n\r\n")
            .expect("HTTP request")
            .0
            .to_ascii_lowercase();
        assert!(headers.starts_with("get /preview.jpeg http/1.1\r\n"));
        assert!(headers.contains("\r\naccept: image/*\r\n"));
        assert!(!headers.contains("\r\nauthorization:"));
    }

    #[tokio::test]
    async fn rejects_untrusted_urls_and_non_image_responses() {
        assert!(matches!(
            call(&Client::new(), &preview("https://example.com/preview.jpeg")).await,
            Err(Error::InvalidPreviewUrl)
        ));

        let (base_url, requests) = serve("200 OK", "text/html", b"<html></html>");
        let error = fetch_from(
            &Client::new(),
            Url::parse(&format!("{base_url}/preview.jpeg")).expect("valid test URL"),
        )
        .await
        .expect_err("content type is rejected");

        assert!(matches!(error, Error::UnexpectedContentType));
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = br#"{"error":"preview unavailable"}"#;
        let (base_url, requests) = serve("404 Not Found", "application/json", body);

        let error = fetch_from(
            &Client::new(),
            Url::parse(&format!("{base_url}/preview.jpeg")).expect("valid test URL"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
        assert_eq!(error.raw_body(), Some(r#"{"error":"preview unavailable"}"#));
        requests.recv().expect("captured request");
    }
}
