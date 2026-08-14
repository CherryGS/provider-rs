pub mod capability;
mod signing;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned credentials for Volcengine OpenAPI request signing.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub access_key_id: &'a SecretString,
    pub secret_access_key: &'a SecretString,
}

/// Caller-owned API key for Ark runtime inference.
#[derive(Clone, Copy, Debug)]
pub struct ArkCredentials<'a> {
    pub api_key: &'a SecretString,
}

impl<'a> ArkCredentials<'a> {
    pub const fn new(api_key: &'a SecretString) -> Self {
        Self { api_key }
    }
}
