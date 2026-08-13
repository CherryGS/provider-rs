//! Caller-managed ChatGPT OAuth for Codex capabilities.
//!
//! This module performs OAuth protocol exchanges but never reads or writes
//! credential storage. Callers own callback handling, token persistence,
//! refresh serialization, and logout policy.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::{Client, StatusCode, Url, header};
use serde::{Deserialize, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{error, fmt, time::Duration};

use crate::{Credentials, ExposeSecret, SecretString};

/// OpenAI's public Codex OAuth client identifier.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

const ISSUER: &str = "https://auth.openai.com";
const SCOPES: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";
const ORIGINATOR: &str = "codex_cli_rs";
const USER_AGENT: &str = concat!("provider-codex/", env!("CARGO_PKG_VERSION"));

/// OAuth tokens returned to caller-owned storage.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tokens {
    #[serde(serialize_with = "serialize_secret")]
    pub id_token: SecretString,
    #[serde(serialize_with = "serialize_secret")]
    pub access_token: SecretString,
    #[serde(serialize_with = "serialize_secret")]
    pub refresh_token: SecretString,
    #[serde(serialize_with = "serialize_secret")]
    pub account_id: SecretString,
}

impl Tokens {
    pub fn credentials(&self) -> Credentials<'_> {
        Credentials::new(&self.access_token, &self.account_id)
    }

    /// Returns the access-token JWT expiry, if the token contains one.
    pub fn expires_at_unix_seconds(&self) -> Result<Option<i64>, Error> {
        let claims: StandardClaims = decode_jwt_payload(self.access_token.expose_secret())?;
        Ok(claims.exp)
    }

    /// Conservatively requests refresh when expiry is absent or within `window`.
    pub fn needs_refresh_at(&self, now_unix_seconds: i64, window: Duration) -> Result<bool, Error> {
        let Some(expires_at) = self.expires_at_unix_seconds()? else {
            return Ok(true);
        };
        let Ok(window) = i64::try_from(window.as_secs()) else {
            return Ok(true);
        };
        Ok(expires_at <= now_unix_seconds.saturating_add(window))
    }
}

fn serialize_secret<S>(value: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value.expose_secret().serialize(serializer)
}

/// One pending Authorization Code + PKCE exchange.
#[derive(Debug)]
pub struct PendingLogin {
    authorization_url: SecretString,
    token_endpoint: String,
    redirect_uri: String,
    client_id: String,
    code_verifier: SecretString,
    state: SecretString,
}

impl PendingLogin {
    pub fn authorization_url(&self) -> &str {
        self.authorization_url.expose_secret()
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn state(&self) -> &str {
        self.state.expose_secret()
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidInput(&'static str),
    StateMismatch,
    InvalidToken(&'static str),
    AccountMismatch,
    Transport(reqwest::Error),
    Response {
        status: StatusCode,
        body: Box<str>,
    },
    Decode {
        source: serde_json::Error,
        body: Box<str>,
    },
}

impl Error {
    pub const fn status(&self) -> Option<StatusCode> {
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
            Self::InvalidInput(field) => write!(formatter, "Codex OAuth {field} is invalid"),
            Self::StateMismatch => formatter.write_str("Codex OAuth state does not match"),
            Self::InvalidToken(field) => {
                write!(formatter, "Codex OAuth token is missing valid {field}")
            }
            Self::AccountMismatch => {
                formatter.write_str("Codex OAuth refresh changed the account identity")
            }
            Self::Transport(_) => formatter.write_str("Codex OAuth request failed"),
            Self::Response { status, .. } => {
                write!(formatter, "Codex OAuth endpoint returned HTTP {status}")
            }
            Self::Decode { .. } => {
                formatter.write_str("Codex OAuth endpoint returned invalid JSON")
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Transport(source) => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Starts a caller-hosted browser login using Authorization Code + PKCE.
pub fn begin_login(redirect_uri: impl Into<String>) -> Result<PendingLogin, Error> {
    begin_login_at(ISSUER, CLIENT_ID, redirect_uri.into())
}

fn begin_login_at(
    issuer: &str,
    client_id: &str,
    redirect_uri: String,
) -> Result<PendingLogin, Error> {
    validate_redirect_uri(&redirect_uri)?;
    if client_id.trim().is_empty() {
        return Err(Error::InvalidInput("client ID"));
    }

    let mut verifier_bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut verifier_bytes);
    let code_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));

    let mut state_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let issuer = issuer.trim_end_matches('/');
    let mut authorization_url = Url::parse(&format!("{issuer}/oauth/authorize"))
        .map_err(|_| Error::InvalidInput("issuer"))?;
    authorization_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("state", &state)
        .append_pair("originator", ORIGINATOR);

    Ok(PendingLogin {
        authorization_url: String::from(authorization_url).into(),
        token_endpoint: format!("{issuer}/oauth/token"),
        redirect_uri,
        client_id: client_id.to_owned(),
        code_verifier: code_verifier.into(),
        state: state.into(),
    })
}

/// Exchanges the callback code after validating the returned OAuth state.
pub async fn exchange_code(
    client: &Client,
    pending: PendingLogin,
    code: &str,
    returned_state: &str,
) -> Result<Tokens, Error> {
    if code.trim().is_empty() {
        return Err(Error::InvalidInput("authorization code"));
    }
    if returned_state != pending.state.expose_secret() {
        return Err(Error::StateMismatch);
    }

    let response = client
        .post(&pending.token_endpoint)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::USER_AGENT, USER_AGENT)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &pending.redirect_uri),
            ("client_id", &pending.client_id),
            ("code_verifier", pending.code_verifier.expose_secret()),
        ])
        .send()
        .await
        .map_err(Error::Transport)?;
    let response: ExchangeResponse = decode_response(response).await?;
    Tokens::from_exchange(response)
}

/// Refreshes tokens without reading or writing caller storage.
///
/// The returned bundle preserves omitted token fields and adopts rotated token
/// fields. Serialize refreshes externally so a refresh token is never reused.
pub async fn refresh(client: &Client, tokens: &Tokens) -> Result<Tokens, Error> {
    refresh_at(client, tokens, &format!("{ISSUER}/oauth/token"), CLIENT_ID).await
}

async fn refresh_at(
    client: &Client,
    tokens: &Tokens,
    endpoint: &str,
    client_id: &str,
) -> Result<Tokens, Error> {
    validate_tokens(tokens)?;
    let response = client
        .post(endpoint)
        .header(header::USER_AGENT, USER_AGENT)
        .json(&RefreshRequest {
            client_id,
            grant_type: "refresh_token",
            refresh_token: tokens.refresh_token.expose_secret(),
        })
        .send()
        .await
        .map_err(Error::Transport)?;
    let response: RefreshResponse = decode_response(response).await?;

    let mut refreshed = tokens.clone();
    if let Some(id_token) = response.id_token {
        if id_token.trim().is_empty() {
            return Err(Error::InvalidToken("ID token"));
        }
        let account_id = account_id(&id_token)?;
        if account_id != refreshed.account_id.expose_secret() {
            return Err(Error::AccountMismatch);
        }
        refreshed.id_token = id_token.into();
    }
    if let Some(access_token) = response.access_token {
        if access_token.trim().is_empty() {
            return Err(Error::InvalidToken("access token"));
        }
        refreshed.access_token = access_token.into();
    }
    if let Some(refresh_token) = response.refresh_token {
        if refresh_token.trim().is_empty() {
            return Err(Error::InvalidToken("refresh token"));
        }
        refreshed.refresh_token = refresh_token.into();
    }
    validate_tokens(&refreshed)?;
    Ok(refreshed)
}

/// Revokes the refresh token. Caller-owned storage remains untouched.
pub async fn revoke(client: &Client, tokens: &Tokens) -> Result<(), Error> {
    revoke_at(client, tokens, &format!("{ISSUER}/oauth/revoke"), CLIENT_ID).await
}

async fn revoke_at(
    client: &Client,
    tokens: &Tokens,
    endpoint: &str,
    client_id: &str,
) -> Result<(), Error> {
    validate_tokens(tokens)?;
    let response = client
        .post(endpoint)
        .header(header::USER_AGENT, USER_AGENT)
        .json(&RevokeRequest {
            token: tokens.refresh_token.expose_secret(),
            token_type_hint: "refresh_token",
            client_id,
        })
        .send()
        .await
        .map_err(Error::Transport)?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(Error::Transport)?
        .into_boxed_str();
    Err(Error::Response { status, body })
}

fn validate_redirect_uri(redirect_uri: &str) -> Result<(), Error> {
    let url = Url::parse(redirect_uri).map_err(|_| Error::InvalidInput("redirect URI"))?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidInput("redirect URI"));
    }
    Ok(())
}

fn validate_tokens(tokens: &Tokens) -> Result<(), Error> {
    for (value, field) in [
        (&tokens.id_token, "ID token"),
        (&tokens.access_token, "access token"),
        (&tokens.refresh_token, "refresh token"),
        (&tokens.account_id, "account ID"),
    ] {
        if value.expose_secret().trim().is_empty() {
            return Err(Error::InvalidToken(field));
        }
    }
    Ok(())
}

impl Tokens {
    fn from_exchange(response: ExchangeResponse) -> Result<Self, Error> {
        let tokens = Self {
            account_id: account_id(&response.id_token)?.into(),
            id_token: response.id_token.into(),
            access_token: response.access_token.into(),
            refresh_token: response.refresh_token.into(),
        };
        validate_tokens(&tokens)?;
        Ok(tokens)
    }
}

fn account_id(id_token: &str) -> Result<String, Error> {
    let claims: IdTokenClaims = decode_jwt_payload(id_token)?;
    claims
        .auth
        .and_then(|auth| {
            auth.get("chatgpt_account_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|account_id| !account_id.trim().is_empty())
        .ok_or(Error::InvalidToken("account ID"))
}

fn decode_jwt_payload<T: DeserializeOwned>(token: &str) -> Result<T, Error> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::InvalidToken("JWT"));
    };
    if header.is_empty() || payload.is_empty() || signature.is_empty() {
        return Err(Error::InvalidToken("JWT"));
    }
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| Error::InvalidToken("JWT"))?;
    serde_json::from_slice(&payload).map_err(|_| Error::InvalidToken("JWT claims"))
}

async fn decode_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, Error> {
    let status = response.status();
    let body = response.bytes().await.map_err(Error::Transport)?;
    let body_text = || String::from_utf8_lossy(&body).into_owned().into_boxed_str();
    if !status.is_success() {
        return Err(Error::Response {
            status,
            body: body_text(),
        });
    }
    serde_json::from_slice(&body).map_err(|source| Error::Decode {
        source,
        body: body_text(),
    })
}

#[derive(Deserialize)]
struct ExchangeResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'static str,
    refresh_token: &'a str,
}

#[derive(Deserialize)]
struct RefreshResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    token: &'a str,
    token_type_hint: &'static str,
    client_id: &'a str,
}

#[derive(Deserialize)]
struct IdTokenClaims {
    #[serde(rename = "https://api.openai.com/auth")]
    auth: Option<Value>,
}

#[derive(Deserialize)]
struct StandardClaims {
    exp: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc::{self, Receiver},
        thread,
    };

    #[test]
    fn begins_pkce_login_without_exposing_verifier() {
        let pending = begin_login(DEFAULT_REDIRECT_URI).expect("valid login");
        let url = Url::parse(pending.authorization_url()).expect("valid authorization URL");
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(
            query.get("client_id").map(|value| value.as_ref()),
            Some(CLIENT_ID)
        );
        assert_eq!(
            query.get("redirect_uri").map(|value| value.as_ref()),
            Some(DEFAULT_REDIRECT_URI)
        );
        assert_eq!(
            query
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );
        assert_eq!(
            query.get("state").map(|value| value.as_ref()),
            Some(pending.state())
        );
        let debug = format!("{pending:?}");
        assert!(!debug.contains(pending.code_verifier.expose_secret()));
        assert!(!debug.contains(pending.state()));
    }

    #[tokio::test]
    async fn rejects_mismatched_state_before_exchange() {
        let pending = begin_login(DEFAULT_REDIRECT_URI).expect("valid login");
        let error = exchange_code(&Client::new(), pending, "code-1", "wrong-state")
            .await
            .expect_err("state mismatch must fail");

        assert!(matches!(error, Error::StateMismatch));
    }

    #[tokio::test]
    async fn exchanges_code_and_extracts_account_id() {
        let id_token = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#);
        let access_token = jwt(r#"{"exp":2000}"#);
        let body = serde_json::json!({
            "id_token": id_token,
            "access_token": access_token,
            "refresh_token": "refresh-1"
        })
        .to_string();
        let (base_url, requests) = serve(&[("200 OK", body)]);
        let pending = begin_login_at(&base_url, "test-client", DEFAULT_REDIRECT_URI.to_owned())
            .expect("valid login");
        let state = pending.state().to_owned();

        let tokens = exchange_code(&Client::new(), pending, "code-1", &state)
            .await
            .expect("exchange succeeds");

        assert_eq!(tokens.account_id.expose_secret(), "account-1");
        assert_eq!(
            tokens.expires_at_unix_seconds().expect("valid JWT"),
            Some(2000)
        );
        let request = requests.recv().expect("captured request");
        assert!(request.starts_with("POST /oauth/token HTTP/1.1\r\n"));
        assert!(request.contains("content-type: application/x-www-form-urlencoded\r\n"));
        let body = request.split_once("\r\n\r\n").expect("request body").1;
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("code=code-1"));
        assert!(body.contains("client_id=test-client"));
    }

    #[tokio::test]
    async fn refreshes_rotated_fields_and_preserves_omitted_fields() {
        let id_token = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#);
        let tokens = Tokens {
            id_token: id_token.into(),
            access_token: jwt(r#"{"exp":1000}"#).into(),
            refresh_token: "refresh-1".into(),
            account_id: "account-1".into(),
        };
        let body = serde_json::json!({
            "access_token": jwt(r#"{"exp":3000}"#),
            "refresh_token": "refresh-2"
        })
        .to_string();
        let (base_url, requests) = serve(&[("200 OK", body)]);

        let refreshed = refresh_at(
            &Client::new(),
            &tokens,
            &format!("{base_url}/oauth/token"),
            "test-client",
        )
        .await
        .expect("refresh succeeds");

        assert_eq!(
            refreshed.id_token.expose_secret(),
            tokens.id_token.expose_secret()
        );
        assert_eq!(refreshed.refresh_token.expose_secret(), "refresh-2");
        assert!(
            !refreshed
                .needs_refresh_at(2000, Duration::from_secs(60))
                .expect("valid JWT")
        );
        let request = requests.recv().expect("captured request");
        assert!(request.contains(r#""grant_type":"refresh_token""#));
        assert!(request.contains(r#""refresh_token":"refresh-1""#));
    }

    #[tokio::test]
    async fn rejects_account_change_during_refresh() {
        let tokens = Tokens {
            id_token: jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#)
                .into(),
            access_token: jwt(r#"{"exp":1000}"#).into(),
            refresh_token: "refresh-1".into(),
            account_id: "account-1".into(),
        };
        let body = serde_json::json!({
            "id_token": jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-2"}}"#),
            "access_token": jwt(r#"{"exp":3000}"#)
        })
        .to_string();
        let (base_url, _requests) = serve(&[("200 OK", body)]);

        let error = refresh_at(
            &Client::new(),
            &tokens,
            &format!("{base_url}/oauth/token"),
            "test-client",
        )
        .await
        .expect_err("account change must fail");

        assert!(matches!(error, Error::AccountMismatch));
    }

    #[tokio::test]
    async fn revokes_refresh_token_without_touching_storage() {
        let tokens = Tokens {
            id_token: jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#)
                .into(),
            access_token: jwt(r#"{"exp":1000}"#).into(),
            refresh_token: "refresh-1".into(),
            account_id: "account-1".into(),
        };
        let (base_url, requests) = serve(&[("200 OK", "{}".to_owned())]);

        revoke_at(
            &Client::new(),
            &tokens,
            &format!("{base_url}/oauth/revoke"),
            "test-client",
        )
        .await
        .expect("revoke succeeds");

        let request = requests.recv().expect("captured request");
        assert!(request.contains(r#""token":"refresh-1""#));
        assert!(request.contains(r#""token_type_hint":"refresh_token""#));
    }

    #[test]
    fn serializes_tokens_without_exposing_debug_output() {
        let tokens = Tokens {
            id_token: "id-secret".into(),
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
            account_id: "account-secret".into(),
        };

        let encoded = serde_json::to_string(&tokens).expect("tokens serialize");
        let decoded: Tokens = serde_json::from_str(&encoded).expect("tokens deserialize");
        assert_eq!(decoded.id_token.expose_secret(), "id-secret");
        assert_eq!(decoded.access_token.expose_secret(), "access-secret");
        assert_eq!(decoded.refresh_token.expose_secret(), "refresh-secret");
        assert_eq!(decoded.account_id.expose_secret(), "account-secret");

        let debug = format!("{tokens:?} {:?}", tokens.credentials());
        for secret in [
            "id-secret",
            "access-secret",
            "refresh-secret",
            "account-secret",
        ] {
            assert!(!debug.contains(secret));
        }
    }

    #[test]
    fn rejects_non_loopback_redirects() {
        assert!(matches!(
            begin_login("https://example.com/callback"),
            Err(Error::InvalidInput("redirect URI"))
        ));
    }

    fn jwt(payload: &str) -> String {
        format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(payload))
    }

    fn serve(responses: &[(&str, String)]) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let responses = responses
            .iter()
            .map(|(status, body)| ((*status).to_owned(), body.clone()))
            .collect::<Vec<_>>();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                let request = read_request(&mut stream);
                sender
                    .send(String::from_utf8(request).expect("UTF-8 request"))
                    .expect("send captured request");
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write response");
            }
        });

        (format!("http://{address}"), receiver)
    }

    fn read_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&request[..header_end]).expect("UTF-8 headers");
            let content_length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        request
    }
}
