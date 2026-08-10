#![allow(clippy::expect_used, clippy::unwrap_used)]

use provider_codex::account_usage::{Credentials, call};
use serde::Deserialize;
use std::{env, fs};

const AUTH_PATH_ENV: &str = "CODEX_AUTH_PATH";

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
async fn fetches_usage_with_env_auth() {
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

    let usage = call(
        &reqwest::Client::new(),
        Credentials {
            access_token: &tokens.access_token,
            account_id,
        },
    )
    .await
    .expect("Codex auth must authorize the account usage endpoint");

    assert!(!usage.plan_type.trim().is_empty());
}
