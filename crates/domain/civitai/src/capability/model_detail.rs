//! Unauthenticated Civitai model detail and preview discovery.
//!
//! Endpoint behavior follows the official
//! [Models API](https://developer.civitai.com/site/reference/models).

use std::{error, fmt};

use reqwest::{Client, StatusCode, header};

use crate::model::Model;

const ENDPOINT: &str = "https://civitai.red/api/v1/models";
const USER_AGENT: &str = concat!("provider-civitai/", env!("CARGO_PKG_VERSION"));

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
            Self::Exchange(_) => formatter.write_str("Civitai model detail request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Civitai model detail returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("Civitai model detail returned invalid JSON")
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

pub async fn call(client: &Client, model_id: u64) -> Result<Model, Error> {
    fetch_at(client, model_id, ENDPOINT).await
}

async fn fetch_at(client: &Client, model_id: u64, endpoint: &str) -> Result<Model, Error> {
    let response = client
        .get(format!("{endpoint}/{model_id}"))
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
mod tests {
    use reqwest::{Client, StatusCode};

    use super::{Error, fetch_at};
    use provider_test_support::serve;

    #[tokio::test]
    async fn requests_model_and_decodes_preview_discovery() {
        let body = br#"{"id":42,"name":"Pony Style","description":"<p>Example</p>","type":"LORA","nsfw":false,"stats":{"downloadCount":12},"creator":{"username":"artist","image":null},"tags":["style"],"modelVersions":[{"id":84,"name":"v1","baseModel":"Pony","publishedAt":"2026-08-10T00:00:00.000Z","files":[{"id":9,"name":"pony.safetensors","type":"Model","sizeKB":12.5,"hashes":{"SHA256":"ABC"},"metadata":{"format":"SafeTensor"},"primary":true}],"images":[{"url":"https://image.civitai.com/preview.jpeg","nsfwLevel":1,"width":512,"height":768,"type":"image"}]}]}"#;
        let (base_url, requests) = serve("200 OK", "application/json", body);

        let model = fetch_at(&Client::new(), 42, &format!("{base_url}/api/v1/models"))
            .await
            .expect("request succeeds");

        assert_eq!(model.name, "Pony Style");
        assert_eq!(
            model.model_versions[0].files[0]
                .hashes
                .get("SHA256")
                .map(String::as_str),
            Some("ABC")
        );
        assert_eq!(
            model.model_versions[0].images[0].url,
            "https://image.civitai.com/preview.jpeg"
        );

        let request = requests.recv().expect("captured request");
        let headers = request
            .split_once("\r\n\r\n")
            .expect("HTTP request")
            .0
            .to_ascii_lowercase();
        assert!(headers.starts_with("get /api/v1/models/42 http/1.1\r\n"));
        assert!(headers.contains("\r\naccept: application/json\r\n"));
        assert!(!headers.contains("\r\nauthorization:"));
    }

    #[tokio::test]
    async fn discovers_video_previews_with_unknown_metadata() {
        let body = br#"{
            "id": 42, "name": "Motion", "type": "LORA",
            "modelVersions": [{
                "id": 84, "name": "v1", "images": [
                    {"url": "https://image.civitai.com/cover.jpeg", "type": "image"},
                    {"url": "https://image.civitai.com/media/original=true/7.mp4",
                     "type": "video", "width": 1024, "height": 576,
                     "meta": {"duration": 3.5}}
                ]
            }]
        }"#;
        let (base_url, requests) = serve("200 OK", "application/json", body);
        let model = fetch_at(&Client::new(), 42, &format!("{base_url}/api/v1/models"))
            .await
            .expect("mixed preview discovery");
        let previews = &model.model_versions[0].images;
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].kind.as_deref(), Some("image"));
        let video: &crate::model::PreviewMedia = &previews[1];
        // The historical type name remains usable with the same response data.
        let legacy: &crate::model::PreviewImage = video;
        assert_eq!(legacy.kind.as_deref(), Some("video"));
        assert_eq!(
            video.url,
            "https://image.civitai.com/media/original=true/7.mp4"
        );
        assert_eq!((video.width, video.height), (Some(1024), Some(576)));
        assert_eq!(video.extra["meta"]["duration"], 3.5);
        requests.recv().expect("captured request");
    }

    #[tokio::test]
    async fn preserves_unsuccessful_status_and_body() {
        let body = br#"{"error":"No model with id 0"}"#;
        let (base_url, requests) = serve("404 Not Found", "application/json", body);

        let error = fetch_at(&Client::new(), 0, &format!("{base_url}/api/v1/models"))
            .await
            .expect_err("request fails");

        assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
        assert_eq!(error.raw_body(), Some(r#"{"error":"No model with id 0"}"#));
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
            let error = fetch_at(&Client::new(), 1, &endpoint)
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
