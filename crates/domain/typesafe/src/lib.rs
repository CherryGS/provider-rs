//! Independently callable TypeSafe API capabilities for Jev.
//!
//! ```no_run
//! use provider_typesafe::{Credentials, SecretString, capability::system_one};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let api_key = SecretString::from(std::env::var("TYPESAFE_API_KEY")?);
//! let request = system_one::Request::new(
//!     "jev-latest",
//!     "The nightly export failed and the report is due this morning.",
//!     [("urgent".into(), system_one::Question::noul("Does this need urgent attention?"))].into(),
//! );
//! let response = system_one::call(
//!     &reqwest::Client::new(),
//!     Credentials::new(&api_key),
//!     &request,
//! ).await?;
//! if let Some(system_one::Answer::Noul(answer)) = response.answers.get("urgent") {
//!     println!("Urgency probability: {}", answer.noul);
//! }
//! # Ok(())
//! # }
//! ```

pub mod capability;
mod client;

pub use client::Client;
pub use secrecy::{ExposeSecret, SecretString};

/// Caller-owned TypeSafe API credentials, supplied explicitly to each call.
#[derive(Clone, Copy, Debug)]
pub struct Credentials<'a> {
    pub api_key: &'a SecretString,
}

impl<'a> Credentials<'a> {
    pub const fn new(api_key: &'a SecretString) -> Self {
        Self { api_key }
    }
}
