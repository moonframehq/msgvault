use {
    crate::{
        Account, AccountId, ArchiveLocation, Attachment, Error, Label, MailboxCounts, MessagePage,
        MessageQuery, MessageSummary, Result, Stats, Thread, http::HttpBackend,
        sqlite::SqliteBackend,
    },
    std::sync::Arc,
};

/// Read access to a msgvault archive.
///
/// Two implementations exist: [`SqliteBackend`] reads `msgvault.db` directly,
/// and [`HttpBackend`] talks to a running `msgvault serve` daemon through its
/// versioned REST API. Every method blocks, so call them off the UI thread.
pub trait Backend: Send + Sync {
    fn describe(&self) -> BackendInfo;
    fn accounts(&self) -> Result<Vec<Account>>;
    fn labels(&self, account: Option<AccountId>) -> Result<Vec<Label>>;
    fn mailbox_counts(&self, account: Option<AccountId>) -> Result<MailboxCounts>;
    fn stats(&self) -> Result<Stats>;
    fn messages(&self, query: &MessageQuery) -> Result<MessagePage>;
    /// The conversation around `message`, oldest first, with bodies.
    fn thread(&self, message: &MessageSummary) -> Result<Thread>;
    /// Attachment bytes, verified against their content hash where known.
    fn attachment(&self, attachment: &Attachment) -> Result<Vec<u8>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Sqlite,
    Daemon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendInfo {
    pub kind: BackendKind,
    /// Database path or daemon URL, for display.
    pub location: String,
}

/// How to pick a backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BackendChoice {
    /// A reachable daemon first, else the database file.
    #[default]
    Auto,
    Sqlite,
    Daemon,
    /// An explicit daemon URL, e.g. `http://nas.local:8080`.
    Url {
        url: String,
        api_key: Option<String>,
    },
}

/// Opens the archive at `location` with the chosen backend.
pub fn open(location: &ArchiveLocation, choice: &BackendChoice) -> Result<Arc<dyn Backend>> {
    match choice {
        BackendChoice::Sqlite => Ok(Arc::new(SqliteBackend::open(location)?)),
        BackendChoice::Daemon => Ok(Arc::new(HttpBackend::discover(location)?)),
        BackendChoice::Url { url, api_key } => {
            Ok(Arc::new(HttpBackend::connect(url, api_key.clone())?))
        },
        BackendChoice::Auto => match HttpBackend::discover(location) {
            Ok(daemon) => Ok(Arc::new(daemon)),
            Err(error) => {
                tracing::debug!(?error, "no msgvault daemon; reading the database directly");
                if location.has_database() {
                    Ok(Arc::new(SqliteBackend::open(location)?))
                } else {
                    Err(Error::ArchiveNotFound {
                        path: location.database_path(),
                    })
                }
            },
        },
    }
}
