//! Errors as the UI receives them: a stable `kind` to switch on, and a message to show.

use caraxyn_core::{error::Error, ids::ParseIdError};
use serde::Serialize;

/// What a failed command returns to the UI.
#[derive(Debug, Serialize)]
pub struct UiError {
    /// The core error's variant name (`AlreadyExists`, `Stalled`, …), or an app-level kind.
    pub kind: &'static str,
    /// Readable text, safe to show as-is.
    pub message: String,
}

impl UiError {
    /// An app-level error (not from the core).
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<Error> for UiError {
    fn from(e: Error) -> Self {
        let kind = match &e {
            Error::Bind(_) => "Bind",
            Error::Store(_) => "Store",
            Error::Io { .. } => "Io",
            Error::InvalidPath => "InvalidPath",
            Error::InvalidPeerId => "InvalidPeerId",
            Error::UnsafeName { .. } => "UnsafeName",
            Error::InvalidInvite(_) => "InvalidInvite",
            Error::ContactRejected => "ContactRejected",
            Error::UnknownShare => "UnknownShare",
            Error::ShareRevoked => "ShareRevoked",
            Error::UnknownOffer => "UnknownOffer",
            Error::Control(_) => "Control",
            Error::IncompatibleData { .. } => "IncompatibleData",
            Error::Stalled => "Stalled",
            Error::Database(_) => "Database",
            Error::InvalidKeyFile(_) => "InvalidKeyFile",
            Error::AlreadyExists(_) => "AlreadyExists",
            _ => "Other",
        };
        Self::new(kind, e.to_string())
    }
}

impl From<ParseIdError> for UiError {
    fn from(e: ParseIdError) -> Self {
        Self::new("InvalidId", e.to_string())
    }
}
