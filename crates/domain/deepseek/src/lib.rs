pub mod capability;
mod client;

pub use client::Client;

#[cfg(test)]
mod test_support;

/// Caller-owned DeepSeek API credentials.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a str,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a str) -> Self {
        Self { api_key }
    }
}
