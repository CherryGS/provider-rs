pub mod capability;
pub use secrecy::{ExposeSecret, SecretString};

#[cfg(test)]
mod test_support;

/// Caller-owned SiliconFlow API credentials.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a SecretString,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a SecretString) -> Self {
        Self { api_key }
    }
}
