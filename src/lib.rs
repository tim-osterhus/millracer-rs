pub mod agent;
pub mod benchmark;
pub mod cli;
pub mod command;
pub mod decision;
pub mod intake;
pub mod millrace;
pub mod monitor;
pub mod operator;
pub mod ops_models;
pub mod ops_service;
pub mod pi;
pub mod pi_rpc;
pub mod prompts;
pub mod scope;
pub mod sessions;
pub mod workspaces;

use std::error::Error;
use std::fmt::{Display, Formatter};

pub type MillracerResult<T> = Result<T, MillracerError>;

#[derive(Debug)]
pub enum MillracerError {
    Message(String),
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl MillracerError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl Display for MillracerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::Json(error) => Display::fmt(error, f),
            Self::Io(error) => Display::fmt(error, f),
        }
    }
}

impl Error for MillracerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Message(_) => None,
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl From<serde_json::Error> for MillracerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<std::io::Error> for MillracerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
