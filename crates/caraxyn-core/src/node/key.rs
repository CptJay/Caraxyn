//! The node's secret key, kept in a file so its identity survives restarts.

use std::{
    fs,
    io::{ErrorKind, Write},
    path::Path,
};

use iroh::SecretKey;

use crate::error::Error;

/// Loads the secret key from `path`, or creates and saves a new one if there
/// is no file yet.
///
/// A file that exists but is not a key is an error and is never replaced:
/// replacing it would silently give this device a new identity.
pub(super) fn load_or_create(path: &Path) -> Result<SecretKey, Error> {
    match fs::read(path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| Error::InvalidKeyFile(path.to_owned()))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let key = SecretKey::generate();
            write_new(path, &key.to_bytes())?;
            Ok(key)
        }
        Err(e) => Err(Error::io(path)(e)),
    }
}

/// Writes `bytes` to `path` readable by the owner only, via a temp file and a
/// rename so a crash never leaves half a key behind.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = path.with_extension("key.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // shortcut: on Windows the file only gets the folder's default access rules (the user profile); setting ACLs needs a dependency or unsafe code.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(Error::io(&tmp))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(Error::io(&tmp))?;
    fs::rename(&tmp, path).map_err(Error::io(path))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_created_key_loads_back() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");

        let created = load_or_create(&path).unwrap();
        let loaded = load_or_create(&path).unwrap();

        assert_eq!(created.to_bytes(), loaded.to_bytes());
        assert_eq!(fs::read(&path).unwrap().len(), 32);
    }

    #[test]
    fn a_damaged_key_file_is_an_error_and_left_alone() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");
        fs::write(&path, b"not a key").unwrap();

        let result = load_or_create(&path);

        assert!(matches!(result, Err(Error::InvalidKeyFile(p)) if p == path));
        assert_eq!(fs::read(&path).unwrap(), b"not a key");
    }

    #[test]
    fn a_leftover_temp_file_does_not_matter() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");
        fs::write(path.with_extension("key.tmp"), b"half").unwrap();

        load_or_create(&path).unwrap();

        assert_eq!(fs::read(&path).unwrap().len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_readable_by_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.key");

        load_or_create(&path).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
