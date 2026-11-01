use std::path::PathBuf;

/// Failures reading a msgvault archive, through either backend.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no msgvault archive found at {}", path.display())]
    ArchiveNotFound { path: PathBuf },
    #[error("unable to open msgvault database {}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("database query failed")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unable to read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to read msgvault config {}", path.display())]
    Config {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("msgvault daemon request to {url} failed")]
    Http {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("no running msgvault daemon was found")]
    DaemonNotFound,
    #[error("message {0} does not exist")]
    MessageNotFound(i64),
    #[error("attachment is stored remotely at {0}")]
    RemoteAttachment(String),
    #[error("attachment content is not stored in the archive")]
    AttachmentMissing,
    #[error("attachment content hash mismatch: expected {expected}, found {actual}")]
    AttachmentCorrupt { expected: String, actual: String },
    #[error("unable to decompress stored data")]
    Decompress(#[source] std::io::Error),
    #[error("stored data exceeds the {limit} byte limit")]
    TooLarge { limit: u64 },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn http(url: impl Into<String>, source: ureq::Error) -> Self {
        Self::Http {
            url: url.into(),
            source: Box::new(source),
        }
    }
}
