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
    /// Reads the settings, or the defaults if there is no file yet.
    pub fn load(data_dir: &Path) -> std::io::Result<Self> {
        match std::fs::read(data_dir.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
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
}
