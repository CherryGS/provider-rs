use reqwest::Client as HttpClient;

use crate::{
    Credentials, SecretString,
    capability::{chat_completions, embeddings, model_list, responses},
};

/// Optional convenience client over the independently callable OpenAI capabilities.
#[derive(Clone)]
pub struct Client {
    http: HttpClient,
    api_key: SecretString,
    organization: Option<SecretString>,
    project: Option<SecretString>,
}

impl Client {
    pub fn new(api_key: impl Into<SecretString>) -> Self {
        Self::with_http(HttpClient::new(), api_key)
    }

    pub fn with_http(http: HttpClient, api_key: impl Into<SecretString>) -> Self {
        Self {
            http,
            api_key: api_key.into(),
            organization: None,
            project: None,
        }
    }

    pub fn with_organization(mut self, organization: impl Into<SecretString>) -> Self {
        self.organization = Some(organization.into());
        self
    }

    pub fn with_project(mut self, project: impl Into<SecretString>) -> Self {
        self.project = Some(project.into());
        self
    }

    pub async fn create_response(
        &self,
        request: &responses::Request,
    ) -> Result<responses::Response, responses::Error> {
        responses::call(&self.http, self.credentials(), request).await
    }

    pub async fn create_chat_completion(
        &self,
        request: &chat_completions::Request,
    ) -> Result<chat_completions::Response, chat_completions::Error> {
        chat_completions::call(&self.http, self.credentials(), request).await
    }

    pub async fn create_embeddings(
        &self,
        request: &embeddings::Request,
    ) -> Result<embeddings::Response, embeddings::Error> {
        embeddings::call(&self.http, self.credentials(), request).await
    }

    pub async fn list_models(&self) -> Result<model_list::Response, model_list::Error> {
        model_list::call(&self.http, self.credentials()).await
    }

    fn credentials(&self) -> Credentials<'_> {
        Credentials {
            api_key: &self.api_key,
            organization: self.organization.as_ref(),
            project: self.project.as_ref(),
        }
    }
}
