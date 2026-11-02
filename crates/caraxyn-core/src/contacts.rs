//! Invite codes: how one device adds another as a contact.
//!
//! A code is `caraxyn1:` followed by base32 text (letters A–Z and digits 2–7),
//! so it survives being pasted into any chat app. It carries the inviter's
//! peer id and name, and a one-time token.
//!
//! ```
//! use caraxyn_core::{contacts::{decode_invite, encode_invite, Invite, InviteToken}, ids::PeerId};
//!
//! let invite = Invite { peer: PeerId::from([7; 32]), name: "Jay".into(), token: InviteToken::random() };
//! let code = encode_invite(&invite).unwrap();
//! assert!(code.starts_with("caraxyn1:"));
//! assert_eq!(decode_invite(&code).unwrap(), invite);
//! ```

use std::fmt;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::PeerId;

/// Start of every invite code; the `1` is the format version.
const PREFIX: &str = "caraxyn1:";

/// How long an invite stays valid after it is issued, in milliseconds (24 hours).
pub const INVITE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Longest display name an invite may carry, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// A random one-time secret that proves the holder was invited.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InviteToken([u8; 16]);

impl InviteToken {
    /// A new token from the operating system's secure random source.
    #[must_use]
    pub fn random() -> Self {
        Self(rand::random())
    }
}

// Not derived: the token is a secret and must not end up in logs.
impl fmt::Debug for InviteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InviteToken(..)")
    }
}

/// What an invite code carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    /// The inviting device.
    pub peer: PeerId,
    /// The inviter's display name, shown to the person invited.
    pub name: String,
    /// The one-time secret the invited device sends back.
    pub token: InviteToken,
}

/// Why an invite code or token was rejected.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InviteError {
    /// The text does not start with `caraxyn1:`.
    #[error("not a Caraxyn invite code (it should start with `caraxyn1:`)")]
    WrongPrefix,
    /// The code is damaged or incomplete.
    #[error("the invite code is damaged or incomplete")]
    Malformed,
    /// The name is empty, too long or contains control characters.
    #[error("the name must be 1 to 64 characters, without control characters")]
    BadName,
    /// The invite is older than 24 hours.
    #[error("the invite has expired")]
    Expired,
    /// The invite was already used.
    #[error("the invite was already used")]
    AlreadyUsed,
}

/// Turns an invite into a code to share.
///
/// # Errors
///
/// [`InviteError::BadName`] if the name could not be decoded on the other side.
pub fn encode_invite(invite: &Invite) -> Result<String, InviteError> {
    check_name(&invite.name)?;
    let bytes = postcard::to_allocvec(invite).expect("an invite always serializes");
    Ok(format!("{PREFIX}{}", BASE32_NOPAD.encode(&bytes)))
}

/// Reads an invite code, for example one pasted from a chat.
///
/// Spaces and line breaks anywhere in the text are ignored, and case does not
/// matter. The content is untrusted, so it is checked strictly.
///
/// # Errors
///
/// [`InviteError::WrongPrefix`], [`InviteError::Malformed`] or
/// [`InviteError::BadName`].
pub fn decode_invite(code: &str) -> Result<Invite, InviteError> {
    let code: String = code
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let body = code
        .strip_prefix(&PREFIX.to_ascii_uppercase())
        .ok_or(InviteError::WrongPrefix)?;
    let bytes = BASE32_NOPAD
        .decode(body.as_bytes())
        .map_err(|_| InviteError::Malformed)?;
    let (invite, rest) =
        postcard::take_from_bytes::<Invite>(&bytes).map_err(|_| InviteError::Malformed)?;
    if !rest.is_empty() {
        return Err(InviteError::Malformed);
    }
    check_name(&invite.name)?;
    Ok(invite)
}

/// Checks whether an issued token may still be used at time `now`.
///
/// Times are unix milliseconds. A token works once, for 24 hours.
///
/// # Errors
///
/// [`InviteError::AlreadyUsed`] or [`InviteError::Expired`].
pub fn check_invite(issued_at: u64, used: bool, now: u64) -> Result<(), InviteError> {
    if used {
        Err(InviteError::AlreadyUsed)
    } else if now >= issued_at.saturating_add(INVITE_TTL_MS) {
        Err(InviteError::Expired)
    } else {
        Ok(())
    }
}

fn check_name(name: &str) -> Result<(), InviteError> {
    let ok = !name.trim().is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.chars().any(char::is_control);
    if ok {
        Ok(())
    } else {
        Err(InviteError::BadName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invite(name: &str) -> Invite {
        Invite {
            peer: PeerId::from([7; 32]),
            name: name.to_owned(),
            token: InviteToken::random(),
        }
    }

    #[test]
    fn a_code_decodes_to_the_same_invite() {
        let invite = invite("Jay");
        assert_eq!(decode_invite(&encode_invite(&invite).unwrap()), Ok(invite));
    }

    #[test]
    fn spaces_line_breaks_and_lowercase_are_tolerated() {
        let invite = invite("Jay");
        let code = encode_invite(&invite).unwrap();
        let (head, tail) = code.split_at(20);
        let pasted = format!("  {head}\n{}  \n", tail.to_lowercase());
        assert_eq!(decode_invite(&pasted), Ok(invite));
    }

    #[test]
    fn bad_codes_say_why() {
        assert_eq!(decode_invite("hello"), Err(InviteError::WrongPrefix));
        assert_eq!(
            decode_invite("caraxyn2:AAAA"),
            Err(InviteError::WrongPrefix)
        );
        assert_eq!(decode_invite("caraxyn1:!!!!"), Err(InviteError::Malformed));
        assert_eq!(decode_invite("caraxyn1:AAAA"), Err(InviteError::Malformed));
        let code = encode_invite(&invite("Jay")).unwrap();
        assert_eq!(
            decode_invite(&code[..code.len() - 4]),
            Err(InviteError::Malformed)
        );
        // Extra bytes after a valid invite are not silently ignored.
        let mut bytes = postcard::to_allocvec(&invite("Jay")).unwrap();
        bytes.push(0);
        let padded = format!("{PREFIX}{}", BASE32_NOPAD.encode(&bytes));
        assert_eq!(decode_invite(&padded), Err(InviteError::Malformed));
    }

    #[test]
    fn names_are_checked_both_ways() {
        for name in [
            "",
            "   ",
            "a\nb",
            "bell\u{7}",
            &"x".repeat(MAX_NAME_CHARS + 1),
        ] {
            assert_eq!(
                encode_invite(&invite(name)),
                Err(InviteError::BadName),
                "{name:?}"
            );
            // A hostile code built without our encoder is rejected too.
            let bytes = postcard::to_allocvec(&invite(name)).unwrap();
            let code = format!("{PREFIX}{}", BASE32_NOPAD.encode(&bytes));
            assert_eq!(decode_invite(&code), Err(InviteError::BadName), "{name:?}");
        }
        let longest = "é".repeat(MAX_NAME_CHARS);
        assert!(encode_invite(&invite(&longest)).is_ok());
    }

    #[test]
    fn a_token_works_once_for_24_hours() {
        let issued = 1_000;
        assert_eq!(check_invite(issued, false, issued), Ok(()));
        assert_eq!(
            check_invite(issued, false, issued + INVITE_TTL_MS - 1),
            Ok(())
        );
        assert_eq!(
            check_invite(issued, false, issued + INVITE_TTL_MS),
            Err(InviteError::Expired)
        );
        assert_eq!(
            check_invite(issued, true, issued),
            Err(InviteError::AlreadyUsed)
        );
    }

    #[test]
    fn tokens_are_random_and_never_printed() {
        let (a, b) = (InviteToken::random(), InviteToken::random());
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "InviteToken(..)");
    }
}
