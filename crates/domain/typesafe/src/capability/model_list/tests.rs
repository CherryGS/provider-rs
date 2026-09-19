#![allow(clippy::expect_used, clippy::unwrap_used)]

use provider_test_support::{serve_json, serve_truncated};
use serde_json::json;

use super::*;

#[tokio::test]
async fn sends_authenticated_get_and_decodes_native_model_envelope() {
    let body = json!({
        "models": [{"name": "jev-latest", "description": "Stable Jev alias", "release_date": "2026-09-15", "future": true}],
        "future": {"version": 2}
    });
    let (base_url, requests) = serve_json("200 OK", body.to_string());
    let key = crate::SecretString::from("test-key");
    let response = fetch_at(
        &Client::new(),
        Credentials::new(&key),
        &format!("{base_url}/v1/models"),
    )
    .await
    .expect("model list");
    assert_eq!(response.models.len(), 1);
    assert_eq!(response.models[0].name, "jev-latest");
    assert_eq!(response.models[0].description, "Stable Jev alias");
    assert_eq!(response.models[0].release_date, "2026-09-15");
    assert_eq!(response.models[0].extra["future"], true);
    assert_eq!(
        serde_json::to_value(response).expect("serialize response"),
        body
    );
    let captured = requests.recv().expect("captured request");
    let (headers, body) = captured.split_once("\r\n\r\n").expect("HTTP request");
    let headers = headers.to_ascii_lowercase();
    assert!(headers.starts_with("get /v1/models http/1.1\r\n"));
    assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
    assert!(headers.contains("\r\naccept: application/json\r\n"));
    assert!(headers.contains(&format!("\r\nuser-agent: {USER_AGENT}\r\n")));
    assert!(body.is_empty());
}

#[tokio::test]
async fn rejects_empty_credentials_before_exchange() {
    for key in ["", " \t\r\n"] {
        let key = crate::SecretString::from(key);
        let error = fetch_at(&Client::new(), Credentials::new(&key), "not a URL")
            .await
            .expect_err("blank API key");
        assert!(matches!(error, Error::InvalidCredentials));
        assert_eq!(error.status(), None);
        assert_eq!(error.raw_body(), None);
    }
}

#[tokio::test]
async fn preserves_http_status_and_body() {
    let key = crate::SecretString::from("test-key");
    let body = r#"{"detail":"sensitive provider message"}"#;
    for (status, expected) in [
        ("401 Unauthorized", StatusCode::UNAUTHORIZED),
        ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let (endpoint, requests) = serve_json(status, body);
        let error = fetch_at(&Client::new(), Credentials::new(&key), &endpoint)
            .await
            .expect_err("provider rejection");
        assert!(matches!(error, Error::Response { .. }));
        assert_eq!(error.status(), Some(expected));
        assert_eq!(error.raw_body(), Some(body));
        assert!(!error.to_string().contains("sensitive provider message"));
        requests.recv().expect("captured request");
    }
}

#[tokio::test]
async fn rejects_malformed_model_envelopes_and_preserves_decode_evidence() {
    let key = crate::SecretString::from("test-key");
    for body in [
        "not JSON",
        r#"{"data":[]}"#,
        r#"{"models":[{"name":"jev-latest"}]}"#,
    ] {
        let (endpoint, requests) = serve_json("200 OK", body);
        let error = fetch_at(&Client::new(), Credentials::new(&key), &endpoint)
            .await
            .expect_err("invalid model envelope");
        assert!(matches!(error, Error::Decode { .. }));
        assert_eq!(error.raw_body(), Some(body));
        assert!(
            std::error::Error::source(&error)
                .and_then(|e| e.downcast_ref::<serde_json::Error>())
                .is_some()
        );
        requests.recv().expect("captured request");
    }
}

#[tokio::test]
async fn preserves_status_and_source_when_body_is_truncated() {
    let key = crate::SecretString::from("test-key");
    for (status, expected) in [
        ("200 OK", StatusCode::OK),
        ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let (endpoint, _requests) = serve_truncated(status, "application/json");
        let error = fetch_at(&Client::new(), Credentials::new(&key), &endpoint)
            .await
            .expect_err("truncated body");
        assert!(matches!(error, Error::BodyRead { .. }));
        assert_eq!(error.status(), Some(expected));
        assert_eq!(error.raw_body(), None);
        assert!(
            std::error::Error::source(&error)
                .and_then(|e| e.downcast_ref::<reqwest::Error>())
                .is_some()
        );
    }
}

#[tokio::test]
async fn preserves_exchange_error_source() {
    let key = crate::SecretString::from("test-key");
    let error = fetch_at(&Client::new(), Credentials::new(&key), "not a URL")
        .await
        .expect_err("invalid URL");
    assert!(matches!(error, Error::Exchange(_)));
    assert_eq!(error.status(), None);
    assert_eq!(error.raw_body(), None);
    assert!(
        std::error::Error::source(&error)
            .and_then(|e| e.downcast_ref::<reqwest::Error>())
            .is_some()
    );
}
