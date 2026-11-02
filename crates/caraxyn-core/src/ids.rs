//! Identifier types used across Caraxyn.
//!
//! [`PeerId`] and [`ContentHash`] are both 32 bytes, written as 64 hex
//! characters, so they share one definition.

use std::{fmt, str::FromStr};

use thiserror::Error;

/// Number of bytes in an id.
const ID_LEN: usize = 32;

/// Why a string could not be parsed as an id.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseIdError {
    /// The string was not exactly 64 characters long.
    #[error("id must be 64 hex characters, got {0}")]
    InvalidLength(usize),
    /// The string contained a character that is not a hex digit.
    #[error("id contains a non-hex character")]
    InvalidHex,
}

fn parse_id(s: &str) -> Result<[u8; ID_LEN], ParseIdError> {
    if s.len() != ID_LEN * 2 {
        return Err(ParseIdError::InvalidLength(s.len()));
    }
    let mut bytes = [0; ID_LEN];
    hex::decode_to_slice(s, &mut bytes).map_err(|_| ParseIdError::InvalidHex)?;
    Ok(bytes)
}

/// Defines a 32-byte id type with hex `Display`/`FromStr`.
macro_rules! hex_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; ID_LEN]);

        impl $name {
            /// First 8 hex characters, short enough for log lines.
            #[must_use]
            pub fn short(&self) -> String {
                hex::encode(&self.0[..4])
            }
        }

        impl From<[u8; ID_LEN]> for $name {
            fn from(bytes: [u8; ID_LEN]) -> Self {
                Self(bytes)
            }
        }

        impl AsRef<[u8; ID_LEN]> for $name {
            fn as_ref(&self) -> &[u8; ID_LEN] {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&hex::encode(self.0))
            }
        }

        // Derived Debug would print 32 separate numbers, which is useless in logs.
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                parse_id(s).map(Self)
            }
        }
    };
}

hex_id! {
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
    PeerId
}

hex_id! {
    /// Identifies shared content: the BLAKE3 hash of a collection.
    ///
    /// Same content always gives the same hash, so the hash is what a receiver
    /// asks for.
    ContentHash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_then_parse_roundtrips() {
        let id = PeerId::from([0xAB; 32]);
        assert_eq!(id.to_string().parse::<PeerId>(), Ok(id));
        let hash = ContentHash::from([0x12; 32]);
        assert_eq!(hash.to_string().parse::<ContentHash>(), Ok(hash));
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
            Err(ParseIdError::InvalidLength(4))
        );
    }

    #[test]
    fn parse_rejects_non_hex() {
        assert_eq!(
            "zz".repeat(32).parse::<PeerId>(),
            Err(ParseIdError::InvalidHex)
        );
    }

    #[test]
    fn short_is_first_eight_chars() {
        assert_eq!(PeerId::from([0xAB; 32]).short(), "abababab");
    }

    #[test]
    fn debug_names_the_type() {
        assert_eq!(
            format!("{:?}", ContentHash::from([0; 32])),
            format!("ContentHash({})", "00".repeat(32))
        );
    }
}
