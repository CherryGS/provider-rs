use reqwest::Client as HttpClient;

use crate::{
    Credentials,
    capability::{chat_completions, model_list, responses},
};

/// Optional convenience client over the independently callable DeepSeek capabilities.
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

    pub async fn create_chat_completion(
        &self,
        request: &chat_completions::Request,
    ) -> Result<chat_completions::Response, chat_completions::Error> {
        chat_completions::call(&self.http, self.credentials(), request).await
    }

    pub async fn create_response(
        &self,
        request: &responses::Request,
    ) -> Result<responses::Response, responses::Error> {
        responses::call(&self.http, self.credentials(), request).await
    }

    pub async fn list_models(&self) -> Result<model_list::Response, model_list::Error> {
        model_list::call(&self.http, self.credentials()).await
    }

    fn credentials(&self) -> Credentials<'_> {
        Credentials {
            api_key: &self.api_key,
        }
    }
}
