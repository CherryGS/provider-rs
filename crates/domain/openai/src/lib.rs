pub mod capability;
mod client;

pub use client::Client;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned OpenAI API invocation credentials and scope.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a SecretString,
    pub organization: Option<&'a SecretString>,
    pub project: Option<&'a SecretString>,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a SecretString) -> Self {
        Self {
            api_key,
            organization: None,
            project: None,
        }
    }
}
