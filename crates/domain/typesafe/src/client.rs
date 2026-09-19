use reqwest::Client as HttpClient;

use crate::{
    Credentials, SecretString,
    capability::{model_list, system_one},
};

/// Optional composition of TypeSafe's independently callable capabilities.
#[derive(Clone)]
pub struct Client {
    http: HttpClient,
    api_key: SecretString,
}

impl Client {
    pub fn new(api_key: impl Into<SecretString>) -> Self {
        Self::with_http(HttpClient::new(), api_key)
    }

    pub fn with_http(http: HttpClient, api_key: impl Into<SecretString>) -> Self {
        Self {
            http,
            api_key: api_key.into(),
        }
    }

    pub async fn system_one(
        &self,
        request: &system_one::Request,
    ) -> Result<system_one::Response, system_one::Error> {
        system_one::call(&self.http, Credentials::new(&self.api_key), request).await
    }

    pub async fn model_list(&self) -> Result<model_list::Response, model_list::Error> {
        model_list::call(&self.http, Credentials::new(&self.api_key)).await
    }
}
