pub mod capability;

#[cfg(test)]
mod test_support;

/// Caller-owned SiliconFlow API credentials.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a str,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a str) -> Self {
        Self { api_key }
    }
}
