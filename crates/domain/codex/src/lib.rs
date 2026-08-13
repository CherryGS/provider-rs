pub mod auth;
mod capability;

pub use capability::account_usage;
pub use capability::model_list;
pub use capability::responses;

/// Caller-owned credentials for one Codex capability invocation.
#[derive(Clone, Copy)]
pub struct Credentials<'a> {
    pub access_token: &'a str,
    pub account_id: &'a str,
}

impl<'a> Credentials<'a> {
    pub const fn new(access_token: &'a str, account_id: &'a str) -> Self {
        Self {
            access_token,
            account_id,
        }
    }
}
