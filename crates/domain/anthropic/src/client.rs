use reqwest::Client as HttpClient;

use crate::{
    Credentials,
    capability::{messages, model_list, token_count},
};

/// Optional convenience client over the independently callable Anthropic capabilities.
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

    pub async fn create_message(
        &self,
        request: &messages::Request,
    ) -> Result<messages::Response, messages::Error> {
        messages::call(&self.http, self.credentials(), request).await
    }

    pub async fn count_message_tokens(
        &self,
        request: &token_count::Request,
    ) -> Result<token_count::Response, token_count::Error> {
        token_count::call(&self.http, self.credentials(), request).await
    }

    pub async fn list_models(
        &self,
        request: &model_list::Request,
    ) -> Result<model_list::Response, model_list::Error> {
        model_list::call(&self.http, self.credentials(), request).await
    }

    fn credentials(&self) -> Credentials<'_> {
        Credentials {
            api_key: &self.api_key,
        }
    }
}
