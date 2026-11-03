//! Crate-wide error type.

use std::path::PathBuf;

use thiserror::Error;

use crate::{contacts::InviteError, safe_name::NameError};

/// Anything that can go wrong in `caraxyn-core`.
///
/// Each module has its own small error type where that makes sense; this
/// enum wraps them (and foreign errors) at the public boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The network endpoint could not be started.
    // shortcut: message only, like Store; keeps iroh out of the public API.
    #[error("failed to start the network endpoint: {0}")]
    Bind(String),
    /// The blob store failed to open, import or write.
    // shortcut: message only, the source chain is lost; split into variants when callers need to match.
    #[error("blob store error: {0}")]
    Store(String),
    /// A filesystem operation failed (for example the shared file does not exist).
    // The reason goes in the message, not as the error's source, so it shows
    // once both in a GUI and in chained output (a field named `source` would
    // become the source automatically).
    #[error("io error at {}: {reason}", path.display())]
    Io {
        /// The file or folder the operation was about.
        path: PathBuf,
        /// What the operating system reported.
        reason: std::io::Error,
    },
    /// The path has no file name that can be shared (or it is not valid UTF-8).
    #[error("path has no usable UTF-8 file name")]
    InvalidPath,
    /// The peer id is not a valid public key.
    #[error("peer id is not a valid public key")]
    InvalidPeerId,
    /// A file name in the share is not safe to write to disk.
    #[error("unsafe file name {name:?}: {reason}")]
    UnsafeName {
        /// The name the sender chose.
        name: String,
        /// Why it was rejected.
        reason: NameError,
    },
    /// The invite code is not valid, or the name given is not.
    #[error("invalid invite: {0}")]
    InvalidInvite(InviteError),
    /// The other device refused the invite: unknown, already used or expired.
    #[error("the invite was refused (it may be used or expired)")]
    ContactRejected,
    /// There is no such offer in the inbox.
    #[error("no such offer in the inbox")]
    UnknownOffer,
    /// A control message to another node could not be sent or answered.
    // shortcut: message only, like Store; split into variants when callers need to match.
    #[error("control message failed: {0}")]
    Control(String),
    /// Caraxyn's own database could not be opened, read or written.
    // shortcut: message only, like Store; split into variants when callers need to match.
    #[error("database error: {0}")]
    Database(String),
    /// The secret key file exists but does not hold a key. It is left as it is.
    #[error("{} is not a valid secret key file", .0.display())]
    InvalidKeyFile(PathBuf),
    /// Exporting would overwrite a file that already exists.
    #[error("{} already exists", .0.display())]
    AlreadyExists(PathBuf),
}

impl Error {
    /// Wraps a blob store error, keeping only its message.
    pub(crate) fn store(e: impl std::fmt::Display) -> Self {
        Self::Store(e.to_string())
    }

    /// Wraps a database error, keeping only its message.
    pub(crate) fn database(e: impl std::fmt::Display) -> Self {
        Self::Database(e.to_string())
    }

    /// For `map_err`: wraps an io error together with the path it happened at.
    pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.to_owned();
        move |reason| Self::Io { path, reason }
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, io::ErrorKind, path::Path};

    use super::*;

    #[test]
    fn the_reason_is_in_the_message_not_the_source() {
        let io = Error::io(Path::new("/x"))(ErrorKind::PermissionDenied.into());
        assert!(io.to_string().contains("permission denied"), "{io}");
        let name = Error::UnsafeName {
            name: "x.".to_owned(),
            reason: NameError::TrailingDotOrSpace,
        };
        for e in [io, name] {
            assert!(e.source().is_none(), "{e}");
        }
    }
}
