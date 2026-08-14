pub mod capability;
mod client;

pub use client::Client;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned DeepSeek API credentials.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a SecretString,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a SecretString) -> Self {
        Self { api_key }
    }
}
