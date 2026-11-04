//! App settings the core does not keep: kept in `settings.json` in the data folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What the user set in the Settings screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The name contacts see in invites; empty until the user sets one.
    pub display_name: String,
    /// Where accepted shares are saved without asking; `None` asks each time.
    pub download_dir: Option<PathBuf>,
}

/// File name inside the data folder.
const FILE: &str = "settings.json";

impl Settings {
    /// Reads the settings, or the defaults if there is no file yet. A file
    /// that is not valid settings is moved to `settings.json.broken` and the
    /// defaults are used, so the next save cannot overwrite what was in it.
    pub fn load(data_dir: &Path) -> std::io::Result<Self> {
        let path = data_dir.join(FILE);
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(settings) => Ok(settings),
                Err(e) => {
                    let aside = data_dir.join(format!("{FILE}.broken"));
                    std::fs::rename(&path, &aside)?;
                    tracing::warn!(%e, path = %aside.display(), "settings unreadable, moved aside");
                    Ok(Self::default())
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes the settings; a crash midway leaves the old file intact.
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let tmp = data_dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, data_dir.join(FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_settings_load_back_and_missing_file_gives_defaults() {
        let dir = tempfile::TempDir::new().unwrap();
        let dir = dir.path();
        assert_eq!(Settings::load(dir).unwrap(), Settings::default());

        let settings = Settings {
            display_name: "Jay".into(),
            download_dir: Some(dir.join("downloads")),
        };
        settings.save(dir).unwrap();
        assert_eq!(Settings::load(dir).unwrap(), settings);
    }

    #[test]
    fn a_broken_file_is_kept_aside_and_defaults_are_used() {
        let dir = tempfile::TempDir::new().unwrap();
        let dir = dir.path();
        std::fs::write(dir.join(FILE), b"{ not json").unwrap();

        assert_eq!(Settings::load(dir).unwrap(), Settings::default());
        Settings::default().save(dir).unwrap();

        let kept = std::fs::read(dir.join("settings.json.broken")).unwrap();
        assert_eq!(kept, b"{ not json");
    }
}
