pub mod capability;
mod signing;

/// Caller-owned credentials for Volcengine OpenAPI request signing.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
}
