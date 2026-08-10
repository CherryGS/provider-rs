use reqwest::Client as HttpClient;

use crate::{Credentials, capability::search};

/// Optional convenience client over the independently callable Exa capabilities.
#[derive(Clone, Debug)]
pub struct Client {
    http: HttpClient,
    api_key: String,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_http(HttpClient::new(), api_key)
    }

    pub fn with_http(http: HttpClient, api_key: impl Into<String>) -> Self {
        Self {
            http,
            api_key: api_key.into(),
        }
    }

    pub async fn search(
        &self,
        request: &search::Request,
    ) -> Result<search::Response, search::Error> {
        search::call(&self.http, self.credentials(), request).await
    }

    fn credentials(&self) -> Credentials<'_> {
        Credentials {
            api_key: &self.api_key,
        }
    }
}
