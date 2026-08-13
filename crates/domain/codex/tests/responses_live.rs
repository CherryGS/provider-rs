#![allow(clippy::expect_used, clippy::unwrap_used)]

use provider_codex::{
    Credentials,
    responses::{Reasoning, Request, call},
};
use serde::Deserialize;
use serde_json::json;
use std::{env, fs};

const AUTH_PATH_ENV: &str = "CODEX_AUTH_PATH";
const MODEL_ENV: &str = "CODEX_RESPONSES_MODEL";

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    account_id: Option<String>,
}

#[tokio::test]
async fn streams_response_with_env_auth() {
    let Some(path) = env::var_os(AUTH_PATH_ENV).filter(|path| !path.is_empty()) else {
        return;
    };
    let bytes = fs::read(path).expect("CODEX_AUTH_PATH must point to a readable auth file");
    let auth: AuthFile =
        serde_json::from_slice(&bytes).expect("CODEX_AUTH_PATH must contain valid Codex auth JSON");
    let tokens = auth
        .tokens
        .expect("Codex auth JSON must contain ChatGPT tokens");
    let account_id = tokens
        .account_id
        .as_deref()
        .filter(|account_id| !account_id.trim().is_empty())
        .expect("Codex auth tokens must contain an account ID");
    assert!(
        !tokens.access_token.trim().is_empty(),
        "Codex auth tokens must contain an access token"
    );

    let model = env::var(MODEL_ENV).unwrap_or_else(|_| "gpt-5.6-sol".to_string());
    let mut request = Request::new(
        model,
        vec![json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Reply with OK."}]
        })],
    );
    request.reasoning = Some(Reasoning {
        effort: Some("low".to_string()),
        summary: None,
        context: Some("all_turns".to_string()),
    });

    let mut events = match call(
        &reqwest::Client::new(),
        Credentials {
            access_token: &tokens.access_token,
            account_id,
        },
        &request,
    )
    .await
    {
        Ok(events) => events,
        Err(error) => panic!("{error}"),
    };

    let mut completed = false;
    loop {
        match events.next().await {
            Ok(Some(event)) => completed |= event.kind == "response.completed",
            Ok(None) => break,
            Err(error) => panic!("{error}"),
        }
    }
    assert!(completed, "Codex Responses stream must complete");
}
