use serde::{Deserialize, Serialize};

pub mod agent_plan_usage;
pub mod chat_completions;
pub mod coding_plan_usage;
pub mod embeddings;
pub mod multimodal_embeddings;

/// Volcengine OpenAPI response metadata shared by the plan-usage actions.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ResponseMetadata {
    pub request_id: Option<String>,
    pub action: Option<String>,
    pub version: Option<String>,
    pub service: Option<String>,
    pub region: Option<String>,
    pub error: Option<ProviderError>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProviderError {
    pub code: String,
    pub message: String,
}

/// Semantic classification of a successful HTTP Volcengine response envelope.
#[derive(Clone, Debug, PartialEq)]
pub enum EndpointOutcome<T> {
    Success { data: T, metadata: ResponseMetadata },
    Partial { data: T, metadata: ResponseMetadata },
    Failure { metadata: ResponseMetadata },
    Malformed { metadata: ResponseMetadata },
}

impl<T> EndpointOutcome<T> {
    fn from_parts(data: Option<T>, metadata: ResponseMetadata) -> Self {
        match (data, metadata.error.is_some()) {
            (Some(data), false) => Self::Success { data, metadata },
            (Some(data), true) => Self::Partial { data, metadata },
            (None, true) => Self::Failure { metadata },
            (None, false) => Self::Malformed { metadata },
        }
    }

    pub const fn metadata(&self) -> &ResponseMetadata {
        match self {
            Self::Success { metadata, .. }
            | Self::Partial { metadata, .. }
            | Self::Failure { metadata }
            | Self::Malformed { metadata } => metadata,
        }
    }

    pub fn provider_error(&self) -> Option<&ProviderError> {
        self.metadata().error.as_ref()
    }

    pub fn provider_code(&self) -> Option<&str> {
        self.provider_error().map(|error| error.code.as_str())
    }

    pub fn provider_message(&self) -> Option<&str> {
        self.provider_error().map(|error| error.message.as_str())
    }

    pub fn request_id(&self) -> Option<&str> {
        self.metadata().request_id.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidCredentials,
    InvalidRequest,
    Encode,
    Clock,
    Signing,
    Transport,
    HttpResponse,
    Decode,
}

/// A provider-reported period whose endpoints are Unix timestamps in milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MillisecondPeriod {
    pub start_ms: i64,
    pub reset_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(error: bool) -> ResponseMetadata {
        ResponseMetadata {
            request_id: Some("request-1".into()),
            action: None,
            version: None,
            service: None,
            region: None,
            error: error.then(|| ProviderError {
                code: "Limited".into(),
                message: "limited".into(),
            }),
        }
    }

    #[test]
    fn classifies_every_response_envelope_combination() {
        assert!(matches!(
            EndpointOutcome::from_parts(Some(1), metadata(false)),
            EndpointOutcome::Success { .. }
        ));
        let partial = EndpointOutcome::from_parts(Some(1), metadata(true));
        assert!(matches!(partial, EndpointOutcome::Partial { .. }));
        assert_eq!(partial.provider_code(), Some("Limited"));
        assert_eq!(partial.provider_message(), Some("limited"));
        assert_eq!(partial.request_id(), Some("request-1"));
        assert!(matches!(
            EndpointOutcome::<i32>::from_parts(None, metadata(true)),
            EndpointOutcome::Failure { .. }
        ));
        assert!(matches!(
            EndpointOutcome::<i32>::from_parts(None, metadata(false)),
            EndpointOutcome::Malformed { .. }
        ));
    }
}
