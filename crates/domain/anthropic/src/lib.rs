pub mod capability;
mod client;

pub use client::Client;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned Anthropic API credentials.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a SecretString,
}
