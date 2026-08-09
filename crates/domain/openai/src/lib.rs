pub mod capability;
mod client;

pub use client::Client;

#[cfg(test)]
mod test_support;

/// Caller-owned OpenAI API invocation credentials and scope.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a str,
    pub organization: Option<&'a str>,
    pub project: Option<&'a str>,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a str) -> Self {
        Self {
            api_key,
            organization: None,
            project: None,
        }
    }
}
