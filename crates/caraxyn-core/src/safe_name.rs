//! Checks for file names that come from other peers.
//!
//! A sender picks the names inside a share, so a name must never be able to
//! leave the folder it is exported to or break on another operating system.

use thiserror::Error;

/// Longest name we accept, in bytes (the limit on common filesystems).
const MAX_LEN: usize = 255;

/// Names Windows refuses (with or without an extension), compared lowercase.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why a file name was rejected.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NameError {
    /// The name is empty.
    #[error("the name is empty")]
    Empty,
    /// The name is `.` or `..`, which would point outside the target folder.
    #[error("`.` and `..` are not allowed")]
    DotName,
    /// The name has a path separator, a control character or a character
    /// Windows forbids.
    #[error("the character {0:?} is not allowed")]
    InvalidChar(char),
    /// The name is reserved by Windows (`CON`, `NUL`, `COM1`, ...).
    #[error("the name is reserved on Windows")]
    Reserved,
    /// The name ends in a dot or a space, which Windows silently strips.
    #[error("the name ends in a dot or a space")]
    TrailingDotOrSpace,
    /// The name is longer than 255 bytes.
    #[error("the name is {0} bytes, the limit is 255")]
    TooLong(usize),
    /// Two entries in the share have this name.
    #[error("another entry has the same name")]
    Duplicate,
}

/// Returns `Ok(())` if `name` is safe to use as a single file name.
pub(crate) fn check(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::DotName);
    }
    if let Some(c) = name.chars().find(|c| {
        c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) {
        return Err(NameError::InvalidChar(c));
    }
    if name.len() > MAX_LEN {
        return Err(NameError::TooLong(name.len()));
    }
    if name.ends_with(['.', ' ']) {
        return Err(NameError::TrailingDotOrSpace);
    }
    let stem = name.split('.').next().unwrap_or(name).to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(NameError::Reserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_pass() {
        for name in [
            "hello.txt",
            "a",
            ".bashrc",
            "my file (1).tar.gz",
            "ünï.txt",
            "console.txt",
        ] {
            assert_eq!(check(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn bad_names_say_why() {
        assert_eq!(check(""), Err(NameError::Empty));
        assert_eq!(check(".."), Err(NameError::DotName));
        assert_eq!(check("."), Err(NameError::DotName));
        assert_eq!(check("a/b"), Err(NameError::InvalidChar('/')));
        assert_eq!(check("../evil"), Err(NameError::InvalidChar('/')));
        assert_eq!(check("a\\b"), Err(NameError::InvalidChar('\\')));
        assert_eq!(check("a\0b"), Err(NameError::InvalidChar('\0')));
        assert_eq!(check("a:b"), Err(NameError::InvalidChar(':')));
        assert_eq!(check("CON"), Err(NameError::Reserved));
        assert_eq!(check("nul.txt"), Err(NameError::Reserved));
        assert_eq!(check("x."), Err(NameError::TrailingDotOrSpace));
        assert_eq!(check("x "), Err(NameError::TrailingDotOrSpace));
        assert_eq!(check(&"a".repeat(256)), Err(NameError::TooLong(256)));
    }
}
