//! Crate-wide error type.

use iroh::endpoint::BindError;
use std::path::PathBuf;

use thiserror::Error;

use crate::safe_name::NameError;

/// Anything that can go wrong in `caraxyn-core`.
///
/// Each module has its own small error type where that makes sense; this
/// enum wraps them (and foreign errors) at the public boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The network endpoint could not be started.
    #[error("failed to start the network endpoint")]
    Bind(#[from] BindError),
    /// The blob store failed to open, import or write.
    // shortcut: message only, the source chain is lost; split into variants when callers need to match.
    #[error("blob store error: {0}")]
    Store(String),
    /// A filesystem operation failed (for example the shared file does not exist).
    #[error("io error at {}", path.display())]
    Io {
        /// The file or folder the operation was about.
        path: PathBuf,
        /// What the operating system reported.
        #[source]
        source: std::io::Error,
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
        #[source]
        reason: NameError,
    },
    /// Exporting would overwrite a file that already exists.
    #[error("{} already exists", .0.display())]
    AlreadyExists(PathBuf),
}

impl Error {
    /// Wraps a blob store error, keeping only its message.
    pub(crate) fn store(e: impl std::fmt::Display) -> Self {
        Self::Store(e.to_string())
    }

    /// For `map_err`: wraps an io error together with the path it happened at.
    pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io { path, source }
    }
}
