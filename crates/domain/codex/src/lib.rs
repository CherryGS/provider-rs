pub mod auth;
mod capability;

pub use capability::account_usage;
pub use capability::model_list;
pub use capability::responses;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned credentials for one Codex capability invocation.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub access_token: &'a SecretString,
    pub account_id: &'a SecretString,
}

impl<'a> Credentials<'a> {
    pub const fn new(access_token: &'a SecretString, account_id: &'a SecretString) -> Self {
        Self {
            access_token,
            account_id,
        }
    }
}
