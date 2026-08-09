pub mod capability;
mod signing;

#[cfg(test)]
mod test_support;

/// Caller-owned credentials for Volcengine OpenAPI request signing.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
}

/// Caller-owned API key for Ark runtime inference.
#[derive(Clone, Copy, Debug)]
pub struct ArkCredentials<'a> {
    pub api_key: &'a str,
}

impl<'a> ArkCredentials<'a> {
    pub const fn new(api_key: &'a str) -> Self {
        Self { api_key }
    }
}
