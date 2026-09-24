//! Retrieval of preview videos discovered in Civitai model responses.
//!
//! Model responses include video entries in `model_versions[].images`, marked
//! with `kind == Some("video")`. The discovered URL is used unchanged; the
//! response content type, not its filename, determines whether it is video.
//! The whole body is buffered in memory. Redirects and timeouts belong to the
//! caller-owned client, as they do for preview images.
//!
//! Discovery follows Civitai's [model response] and [media URL builder].
//!
//! [model response]: https://github.com/civitai/civitai/blob/main/src/pages/api/v1/models/%5Bid%5D.ts
//! [media URL builder]: https://github.com/civitai/civitai/blob/main/src/client-utils/edge-url.ts
//!
//! ```no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use provider_civitai::capability::{model_detail, preview_video};
//!
//! let client = reqwest::Client::new();
//! let model = model_detail::call(&client, 410919).await?;
//! for preview in model.model_versions.iter().flat_map(|version| &version.images) {
//!     if preview.kind.as_deref() == Some("video") {
//!         let video = preview_video::call(&client, preview).await?;
//!         // The caller chooses where to save or play `video.bytes`.
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::{error, fmt};

use bytes::Bytes;
use reqwest::{Client, StatusCode, Url, header};

use crate::model::PreviewMedia;

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
    /// Reading the response body failed after receiving the HTTP status.
    BodyRead {
        status: StatusCode,
        source: reqwest::Error,
    },
    Response {
        status: StatusCode,
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
            Self::Response { body, .. } => Some(body),
            _ => None,
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
            Self::InvalidPreviewUrl => {
                formatter.write_str("Civitai preview URL is not an official HTTPS media URL")
            }
            Self::UnexpectedContentType => {
                formatter.write_str("Civitai preview response is not a video")
            }
            Self::Exchange(_) => formatter.write_str("Civitai preview video request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Civitai preview video returned HTTP {status}")
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::BodyRead { source, .. } => Some(source),
            Self::Exchange(source) => Some(source),
            _ => None,
        }
    }
}

pub async fn call(client: &Client, preview: &PreviewMedia) -> Result<Response, Error> {
    let url = validate_url(&preview.url)?;
    fetch_from(client, url).await
}

async fn fetch_from(client: &Client, url: Url) -> Result<Response, Error> {
    let response = client
        .get(url)
        .header(header::ACCEPT, "video/*")
        .header(header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(Error::Exchange)?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .bytes()
            .await
            .map_err(|source| Error::BodyRead { status, source })?;
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
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"video/"))
        })
        .ok_or(Error::UnexpectedContentType)?
        .to_owned();
    let bytes = response
        .bytes()
        .await
        .map_err(|source| Error::BodyRead { status, source })?;

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
    use provider_test_support::serve;

    use crate::model::PreviewMedia;

    fn preview(url: impl Into<String>) -> PreviewMedia {
        PreviewMedia {
            id: Some(7),
            url: url.into(),
            nsfw_level: Some(1),
            width: Some(512),
            height: Some(768),
            hash: None,
            kind: Some("video".to_owned()),
            extra: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn requests_discovered_video_and_returns_binary_body() {
        let body = [0, 0, 0, 0x18, b'f', b't', b'y', b'p'];
        let (base_url, requests) = serve("200 OK", "video/mp4", body);

        let response = fetch_from(
            &Client::new(),
            Url::parse(&format!("{base_url}/preview.mp4")).expect("valid test URL"),
        )
        .await
        .expect("request succeeds");

        assert_eq!(response.content_type, "video/mp4");
        assert_eq!(response.bytes.as_ref(), body);

        let request = requests.recv().expect("captured request");
        let headers = request
            .split_once("\r\n\r\n")
            .expect("HTTP request")
            .0
            .to_ascii_lowercase();
        assert!(headers.starts_with("get /preview.mp4 http/1.1\r\n"));
        assert!(headers.contains("\r\naccept: video/*\r\n"));
        assert!(!headers.contains("\r\nauthorization:"));
    }

    #[tokio::test]
    async fn rejects_untrusted_urls_before_requesting() {
        for url in [
            "not a URL",
            "https://example.com/preview.mp4",
            "http://image.civitai.com/preview.mp4",
            "https://image.civitai.com:8443/preview.mp4",
            "https://user:secret@image.civitai.com/preview.mp4",
            "https://user@image.civitai.com/preview.mp4",
            "https://image.civitai.com.example.com/preview.mp4",
        ] {
            assert!(matches!(
                call(&Client::new(), &preview(url)).await,
                Err(Error::InvalidPreviewUrl)
            ));
        }
    }

    #[test]
    fn accepts_official_video_urls_without_rewriting() {
        let url = "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/media/original=true/7.mp4";
        assert_eq!(
            super::validate_url(url).expect("official URL").as_str(),
            url
        );
    }

    #[tokio::test]
    async fn rejects_images_and_other_non_video_responses() {
        for content_type in ["image/jpeg", "text/html", "application/octet-stream", ""] {
            let (base_url, requests) = serve("200 OK", content_type, b"not video");
            let error = fetch_from(
                &Client::new(),
                Url::parse(&format!("{base_url}/preview.mp4")).expect("valid test URL"),
            )
            .await
            .expect_err("content type is rejected");
            assert!(matches!(error, Error::UnexpectedContentType));
            requests.recv().expect("captured request");
        }
    }

    #[tokio::test]
    async fn accepts_video_content_types_regardless_of_filename() {
        for content_type in ["video/webm", "Video/MP4; charset=binary"] {
            let (base_url, requests) = serve("200 OK", content_type, b"video bytes");
            let response = fetch_from(
                &Client::new(),
                Url::parse(&format!("{base_url}/preview.jpeg")).expect("valid test URL"),
            )
            .await
            .expect("video content type");
            assert_eq!(response.content_type, content_type);
            assert_eq!(response.bytes.as_ref(), b"video bytes");
            requests.recv().expect("captured request");
        }
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = br#"{"error":"preview unavailable"}"#;
        let (base_url, requests) = serve("404 Not Found", "application/json", body);

        let error = fetch_from(
            &Client::new(),
            Url::parse(&format!("{base_url}/preview.mp4")).expect("valid test URL"),
        )
        .await
        .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
        assert_eq!(error.raw_body(), Some(r#"{"error":"preview unavailable"}"#));
        assert!(!error.to_string().contains("preview unavailable"));
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn preserves_status_when_response_body_is_truncated() {
        for (status, expected) in [
            ("200 OK", StatusCode::OK),
            ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
        ] {
            let (endpoint, _requests) =
                provider_test_support::serve_truncated(status, "video/webm");
            let error = fetch_from(&Client::new(), Url::parse(&endpoint).expect("test URL"))
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
