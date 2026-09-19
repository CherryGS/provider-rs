//! TypeSafe System One evaluation, including Jev's Choice, Score, and Noul questions.
//!
//! Wire contract: <https://docs.typesafe.ai/api>. Structured descriptions follow
//! <https://docs.typesafe.ai/primitives/advanced> and the official JavaScript SDK.

use std::{collections::BTreeMap, error, fmt};

use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Credentials, ExposeSecret};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const USER_AGENT: &str = concat!("provider-typesafe/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    /// A versioned model ID or alias such as `jev-latest`.
    pub model: String,
    /// Text, a JSON object or array, or null, as accepted by the JavaScript SDK.
    pub state: Value,
    /// Each answer is returned under the corresponding caller-chosen key.
    pub questions: BTreeMap<String, Question>,
}

impl Request {
    pub fn new(
        model: impl Into<String>,
        state: impl Into<Value>,
        questions: BTreeMap<String, Question>,
    ) -> Self {
        Self {
            model: model.into(),
            state: state.into(),
            questions,
        }
    }
}

/// Instructions and descriptions accept text, objects, arrays, or null.
/// Nested JSON values may also contain numbers and booleans.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: Value,
        /// Up to 255 named options; null leaves an option undescribed.
        criteria: BTreeMap<String, Value>,
    },
    Score {
        instructions: Value,
        /// Two to ten ordered levels, numbered from zero.
        criteria: Vec<Value>,
    },
    Noul {
        instructions: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
}

impl Question {
    pub fn choice(instructions: impl Into<Value>, criteria: BTreeMap<String, Value>) -> Self {
        Self::Choice {
            instructions: instructions.into(),
            criteria,
        }
    }

    pub fn score(instructions: impl Into<Value>, criteria: Vec<Value>) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria,
        }
    }

    pub fn noul(instructions: impl Into<Value>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true", skip_serializing_if = "Option::is_none")]
    pub when_true: Option<Value>,
    #[serde(rename = "false", skip_serializing_if = "Option::is_none")]
    pub when_false: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Response {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    Noul(NoulAnswer),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ScoreAnswer {
    /// Provider-reported weighted score; it may lie between integer levels.
    pub score: f64,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
    /// Level indices mapped to the original, possibly structured descriptions.
    pub legend: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct NoulAnswer {
    /// Probability of yes, retained without applying a decision threshold.
    pub noul: f64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Usage {
    /// Missing or null counters remain unknown, rather than becoming zero.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub enum Error {
    InvalidCredentials,
    InvalidRequest(&'static str),
    Exchange(reqwest::Error),
    /// Reading the response body failed after receiving the HTTP status.
    BodyRead {
        status: StatusCode,
        source: reqwest::Error,
    },
    Response {
        status: StatusCode,
        body: String,
    },
    Decode {
        source: serde_json::Error,
        body: String,
    },
}

impl Error {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Response { status, .. } | Self::BodyRead { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn raw_body(&self) -> Option<&str> {
        match self {
            Self::Response { body, .. } | Self::Decode { body, .. } => Some(body),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => formatter.write_str("TypeSafe API key is empty"),
            Self::InvalidRequest(field) => {
                write!(formatter, "TypeSafe System One field `{field}` is invalid")
            }
            Self::Exchange(_) => formatter.write_str("TypeSafe System One request failed"),
            Self::BodyRead { status, .. } => write!(
                formatter,
                "TypeSafe System One response body read failed after HTTP {status}"
            ),
            Self::Response { status, .. } => {
                write!(formatter, "TypeSafe System One returned HTTP {status}")
            }
            Self::Decode { .. } => formatter.write_str("TypeSafe System One returned invalid JSON"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Exchange(source) | Self::BodyRead { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Evaluates all questions in one request, without adding an automatic retry loop.
pub async fn call(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
) -> Result<Response, Error> {
    evaluate_at(client, credentials, request, ENDPOINT).await
}

async fn evaluate_at(
    client: &Client,
    credentials: Credentials<'_>,
    request: &Request,
    endpoint: &str,
) -> Result<Response, Error> {
    validate(credentials, request)?;

    let response = client
        .post(endpoint)
        .bearer_auth(credentials.api_key.expose_secret())
        .header(header::ACCEPT, "application/json")
        .header(header::USER_AGENT, USER_AGENT)
        .json(request)
        .send()
        .await
        .map_err(Error::Exchange)?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|source| Error::BodyRead { status, source })?;
    if !status.is_success() {
        return Err(Error::Response {
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }

    serde_json::from_slice(&body).map_err(|source| Error::Decode {
        source,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn validate(credentials: Credentials<'_>, request: &Request) -> Result<(), Error> {
    if credentials.api_key.expose_secret().trim().is_empty() {
        return Err(Error::InvalidCredentials);
    }
    if request.model.trim().is_empty() {
        return Err(Error::InvalidRequest("model"));
    }
    if !is_entry(&request.state) {
        return Err(Error::InvalidRequest("state"));
    }
    if request.questions.is_empty() {
        return Err(Error::InvalidRequest("questions"));
    }
    for question in request.questions.values() {
        let instructions = match question {
            Question::Choice {
                instructions,
                criteria,
            } => {
                if !(1..=255).contains(&criteria.len()) || !criteria.values().all(is_entry) {
                    return Err(Error::InvalidRequest("questions.choice.criteria"));
                }
                instructions
            }
            Question::Score {
                instructions,
                criteria,
            } => {
                if !(2..=10).contains(&criteria.len()) || !criteria.iter().all(is_entry) {
                    return Err(Error::InvalidRequest("questions.score.criteria"));
                }
                instructions
            }
            Question::Noul {
                instructions,
                criteria,
            } => {
                if let Some(criteria) = criteria
                    && !criteria
                        .when_true
                        .iter()
                        .chain(criteria.when_false.iter())
                        .all(is_entry)
                {
                    return Err(Error::InvalidRequest("questions.noul.criteria"));
                }
                instructions
            }
        };
        if !is_entry(instructions) {
            return Err(Error::InvalidRequest("questions.instructions"));
        }
    }
    Ok(())
}

fn is_entry(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::String(_) | Value::Object(_) | Value::Array(_)
    )
}

#[cfg(test)]
mod tests;
