//! Locating a msgvault archive the way msgvault itself does
//! (`internal/config/config.go`): `$MSGVAULT_HOME`, else `~/.msgvault`, with an
//! optional `config.toml` that may move the data directory or add an API key.

use {
    crate::{Error, Result},
    serde::Deserialize,
    std::path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchiveLocation {
    /// msgvault's home directory, holding `config.toml`.
    pub home: PathBuf,
    /// `[data] data_dir`: database, attachments and daemon runtime records.
    pub data_dir: PathBuf,
    /// `[server] api_key` for the local daemon, if configured.
    pub api_key: Option<String>,
    /// `[server] api_port`, when fixed rather than random.
    pub api_port: Option<u16>,
    /// `[remote] url`, a daemon on another machine.
    pub remote_url: Option<String>,
    pub remote_api_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    data: DataSection,
    #[serde(default)]
    server: ServerSection,
    #[serde(default)]
    remote: RemoteSection,
}

#[derive(Debug, Default, Deserialize)]
struct DataSection {
    data_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ServerSection {
    api_key: Option<String>,
    api_port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
struct RemoteSection {
    url: Option<String>,
    api_key: Option<String>,
}

impl ArchiveLocation {
    /// `$MSGVAULT_HOME`, else `~/.msgvault`.
    pub fn default_home() -> PathBuf {
        if let Some(home) = std::env::var_os("MSGVAULT_HOME").filter(|v| !v.is_empty()) {
            return expand_tilde(Path::new(&home));
        }
        home_dir().join(".msgvault")
    }

    /// Reads `<home>/config.toml` if present. A missing file is not an error.
    pub fn load(home: impl AsRef<Path>) -> Result<Self> {
        let home = expand_tilde(home.as_ref());
        let config_path = home.join("config.toml");
        let config = match std::fs::read_to_string(&config_path) {
            Ok(text) => toml::from_str::<ConfigFile>(&text).map_err(|source| Error::Config {
                path: config_path.clone(),
                source,
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ConfigFile::default(),
            Err(error) => return Err(Error::io(config_path, error)),
        };
        let data_dir = config
            .data
            .data_dir
            .filter(|dir| !dir.trim().is_empty())
            .map(|dir| expand_tilde(Path::new(&dir)))
            .unwrap_or_else(|| home.clone());
        let non_empty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
        Ok(Self {
            home,
            data_dir,
            api_key: non_empty(config.server.api_key),
            api_port: config.server.api_port.filter(|port| *port != 0),
            remote_url: non_empty(config.remote.url),
            remote_api_key: non_empty(config.remote.api_key),
        })
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("msgvault.db")
    }

    pub fn attachments_dir(&self) -> PathBuf {
        self.data_dir.join("attachments")
    }

    pub fn has_database(&self) -> bool {
        self.database_path().is_file()
    }
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home_dir().join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// Shortens a path under the user's home to `~/...` for display.
pub fn display_path(path: &Path) -> String {
    let home = home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        Ok(_) => "~".into(),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_defaults_to_home() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| Error::io("tempdir", e))?;
        let location = ArchiveLocation::load(dir.path())?;
        assert_eq!(location.data_dir, dir.path());
        assert_eq!(location.database_path(), dir.path().join("msgvault.db"));
        assert!(!location.has_database());
        Ok(())
    }

    #[test]
    fn config_moves_data_and_sets_keys() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| Error::io("tempdir", e))?;
        std::fs::write(
            dir.path().join("config.toml"),
            "[data]\ndata_dir = \"/srv/vault\"\n[server]\napi_key = \"k\"\napi_port = 8080\n",
        )
        .map_err(|e| Error::io("config", e))?;
        let location = ArchiveLocation::load(dir.path())?;
        assert_eq!(location.data_dir, PathBuf::from("/srv/vault"));
        assert_eq!(location.api_key.as_deref(), Some("k"));
        assert_eq!(location.api_port, Some(8080));
        Ok(())
    }

    #[test]
    fn malformed_config_is_a_typed_error() {
        let Ok(dir) = tempfile::tempdir() else {
            return;
        };
        let _ = std::fs::write(dir.path().join("config.toml"), "[data\n");
        assert!(matches!(
            ArchiveLocation::load(dir.path()),
            Err(Error::Config { .. })
        ));
    }
}
