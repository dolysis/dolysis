use std::fmt::Display;
use {
    serde::{Deserialize, Serialize},
    std::{error, fmt},
};

/// Simple error struct that contains an approximate time
/// at which the error occurred, an error kind, and the
/// textual message of the original error.
#[derive(Debug, Serialize, Deserialize)]
pub struct CrateError {
    time: i64,
    kind: Kind,
    msg: String,
}

impl CrateError {
    pub fn new<E>(time: i64, kind: Option<Kind>, msg: E) -> Self
    where
        E: error::Error,
    {
        Self {
            time,
            kind: kind.unwrap_or_default(),
            msg: msg.to_string(),
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn timestamp_nanos(&self) -> i64 {
        self.time
    }
}

impl Display for CrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} error as occurred at nano-second epoch {} with the message: {}",
            self.kind, self.time, self.msg
        )
    }
}

impl error::Error for CrateError {}

/// Catagories of error
// Expand when needed
// TODO: make #[non-exhaustive] once rust > 1.40
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    Generic,
}

impl Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Generic => "Generic",
        };

        write!(f, "{}", s)
    }
}

impl Default for Kind {
    fn default() -> Self {
        Self::Generic
    }
}
