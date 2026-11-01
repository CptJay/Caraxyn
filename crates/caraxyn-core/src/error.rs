//! Crate-wide error type.

use iroh::endpoint::BindError;
use thiserror::Error;

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
}
