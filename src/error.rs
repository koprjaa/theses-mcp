// Project: theses-mcp
// File: src/error.rs
// Description: Error type that carries the Python exception name some results print.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
use std::fmt;

/// `class` is the name the Python server printed for the same failure, for example
/// `ConnectionError` or `ReadTimeout`, because tool results quote it.
#[derive(Debug, Clone)]
pub struct Error {
    pub class: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Error {
            class,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
