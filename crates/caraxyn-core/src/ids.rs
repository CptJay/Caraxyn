//! Identifier types used across Caraxyn.

use std::{fmt, str::FromStr};

use thiserror::Error;

/// Number of bytes in a [`PeerId`] (an ed25519 public key).
const PEER_ID_LEN: usize = 32;

/// Identifies a Caraxyn device: its 32-byte public key.
///
/// This is Caraxyn's own type; conversions to and from iroh's id type live
/// at the iroh boundary so the rest of the code never depends on it.
///
/// Text form is 64 lowercase hex characters:
///
/// ```
/// use caraxyn_core::ids::PeerId;
///
/// let id = PeerId::from([7; 32]);
/// let text = id.to_string();
/// assert_eq!(text.parse::<PeerId>().unwrap(), id);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId([u8; PEER_ID_LEN]);

impl PeerId {
    /// First 8 hex characters, short enough for log lines.
    #[must_use]
    pub fn short(&self) -> String {
        hex::encode(&self.0[..4])
    }
}

impl From<[u8; PEER_ID_LEN]> for PeerId {
    fn from(bytes: [u8; PEER_ID_LEN]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8; PEER_ID_LEN]> for PeerId {
    fn as_ref(&self) -> &[u8; PEER_ID_LEN] {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

// Derived Debug would print 32 separate numbers, which is useless in logs.
impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId({self})")
    }
}

/// Why a string could not be parsed as a [`PeerId`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParsePeerIdError {
    /// The string was not exactly 64 characters long.
    #[error("peer id must be 64 hex characters, got {0}")]
    InvalidLength(usize),
    /// The string contained a character that is not a hex digit.
    #[error("peer id contains a non-hex character")]
    InvalidHex,
}

impl FromStr for PeerId {
    type Err = ParsePeerIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        
        // Helper to avoid repeating the length check in the error message.
        fn is_correct_length(s: &str) -> bool {
            s.len() == PEER_ID_LEN * 2
        }

        if !is_correct_length(s) {
            return Err(ParsePeerIdError::InvalidLength(s.len()));
        }
        let mut bytes = [0; PEER_ID_LEN];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| ParsePeerIdError::InvalidHex)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_then_parse_roundtrips() {
        let id = PeerId::from([0xAB; 32]);
        assert_eq!(id.to_string().parse::<PeerId>(), Ok(id));
    }

    #[test]
    fn display_is_lowercase_hex() {
        assert_eq!(PeerId::from([0xAB; 32]).to_string(), "ab".repeat(32));
    }

    #[test]
    fn parse_accepts_uppercase() {
        assert_eq!(
            "AB".repeat(32).parse::<PeerId>(),
            Ok(PeerId::from([0xAB; 32]))
        );
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert_eq!(
            "abcd".parse::<PeerId>(),
            Err(ParsePeerIdError::InvalidLength(4))
        );
    }

    #[test]
    fn parse_rejects_non_hex() {
        assert_eq!(
            "zz".repeat(32).parse::<PeerId>(),
            Err(ParsePeerIdError::InvalidHex)
        );
    }

    #[test]
    fn short_is_first_eight_chars() {
        assert_eq!(PeerId::from([0xAB; 32]).short(), "abababab");
    }
}
