//! Direct, read-only access to `msgvault.db`.
//!
//! msgvault keeps its database in WAL mode. Like msgvault's own read-only
//! store (`internal/store/store.go` `OpenReadOnly`), connections open
//! read-write with `PRAGMA query_only` rather than `SQLITE_OPEN_READONLY`,
//! because a WAL reader may need to create the `-shm` index file.

mod blobs;
mod filter;
mod raw;
mod rows;

pub(crate) use blobs::{ATTACHMENT_LIMIT, content_hash, hex};
use {
    crate::{
        Account, AccountId, ArchiveLocation, Attachment, Backend, BackendInfo, BackendKind, Error,
        Label, Mailbox, MailboxCounts, MessageId, MessagePage, MessageQuery, MessageSummary,
        Result, Stats, Thread, location::display_path,
    },
    filter::Filter,
    rusqlite::{Connection, OpenFlags},
    std::{
        path::{Path, PathBuf},
        sync::{
            Mutex, MutexGuard,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    },
};

/// Connections kept open, so a slow count never blocks opening a message.
const POOL_SIZE: usize = 3;
/// Messages shown before and after the selected one in a conversation.
const THREAD_BEFORE: usize = 40;
const THREAD_AFTER: usize = 10;

/// Optional schema features; archives upgraded from older msgvault releases
/// may lack newer columns and tables.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Schema {
    pub fts: bool,
    pub imap_flags: bool,
    pub pack_index: bool,
    pub label_system_role: bool,
    pub attachment_role: bool,
    pub attachment_content_id: bool,
}

pub struct SqliteBackend {
    path: PathBuf,
    attachments_dir: PathBuf,
    schema: Schema,
    pool: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl SqliteBackend {
    pub fn open(location: &ArchiveLocation) -> Result<Self> {
        Self::open_paths(location.database_path(), location.attachments_dir())
    }

    pub fn open_paths(
        path: impl Into<PathBuf>,
        attachments_dir: impl Into<PathBuf>,
    ) -> Result<Self> {
        let path = path.into();
        if !path.is_file() {
            return Err(Error::ArchiveNotFound { path });
        }
        let pool = (0..POOL_SIZE)
            .map(|_| connect(&path).map(Mutex::new))
            .collect::<Result<Vec<_>>>()?;
        let schema = match pool.first() {
            Some(conn) => probe(&lock(conn))?,
            None => Schema::default(),
        };
        tracing::info!(path = %path.display(), ?schema, "opened msgvault database");
        Ok(Self {
            path,
            attachments_dir: attachments_dir.into(),
            schema,
            pool,
            next: AtomicUsize::new(0),
        })
    }

    fn with<T>(&self, run: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if let Some(conn) = self.pool.iter().find_map(|conn| conn.try_lock().ok()) {
            return run(&conn);
        }
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.pool.len().max(1);
        match self.pool.get(index) {
            Some(conn) => run(&lock(conn)),
            None => Err(Error::ArchiveNotFound {
                path: self.path.clone(),
            }),
        }
    }

    fn filter<'c>(&self, conn: &'c Connection, account: Option<AccountId>) -> Filter<'c> {
        Filter::new(conn, self.schema, account)
    }
}

fn lock(conn: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    // A panic while holding a read-only connection leaves nothing half-written.
    conn.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn connect(path: &Path) -> Result<Connection> {
    let base = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(path, base | OpenFlags::SQLITE_OPEN_READ_WRITE)
        .or_else(|_| Connection::open_with_flags(path, base | OpenFlags::SQLITE_OPEN_READ_ONLY))
        .map_err(|source| Error::Open {
            path: path.to_path_buf(),
            source,
        })?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

fn probe(conn: &Connection) -> Result<Schema> {
    let table = |name: &str| -> Result<bool> {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )? > 0)
    };
    let column = |table: &str, column: &str| -> Result<bool> {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            [table, column],
            |row| row.get::<_, i64>(0),
        )? > 0)
    };
    // The FTS5 module must also load, not merely be declared.
    let fts = table("messages_fts")?
        && conn
            .query_row("SELECT 1 FROM messages_fts LIMIT 1", [], |_| Ok(()))
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(()),
                other => Err(other),
            })
            .is_ok();
    Ok(Schema {
        fts,
        imap_flags: table("imap_message_memberships")?,
        pack_index: table("attachment_pack_index")?,
        label_system_role: column("labels", "system_role")?,
        attachment_role: column("attachments", "attachment_role")?,
        attachment_content_id: column("attachments", "content_id")?,
    })
}

impl Backend for SqliteBackend {
    fn describe(&self) -> BackendInfo {
        BackendInfo {
            kind: BackendKind::Sqlite,
            location: display_path(&self.path),
        }
    }

    fn accounts(&self) -> Result<Vec<Account>> {
        self.with(rows::accounts)
    }

    fn labels(&self, account: Option<AccountId>) -> Result<Vec<Label>> {
        self.with(|conn| rows::labels(conn, account))
    }

    fn mailbox_counts(&self, account: Option<AccountId>) -> Result<MailboxCounts> {
        self.with(|conn| {
            let count = |mailbox: Mailbox| -> Result<u64> {
                let mut filter = self.filter(conn, account);
                filter.mailbox(&mailbox)?;
                filter.count()
            };
            let mut unread = self.filter(conn, account);
            unread.mailbox(&Mailbox::Inbox)?;
            unread.mailbox(&Mailbox::Unread)?;
            let kinds = rows::kinds(conn, account)?;
            let all_mail = kinds
                .iter()
                .find(|(kind, _)| kind == "email")
                .map_or(0, |(_, count)| *count);
            Ok(MailboxCounts {
                inbox_unread: unread.count()?,
                inbox: count(Mailbox::Inbox)?,
                starred: count(Mailbox::Starred)?,
                drafts: count(Mailbox::Drafts)?,
                all_mail,
                attachments: count(Mailbox::Attachments)?,
                removed_upstream: count(Mailbox::RemovedUpstream)?,
                kinds: kinds
                    .into_iter()
                    .filter(|(kind, _)| kind != "email")
                    .collect(),
            })
        })
    }

    fn stats(&self) -> Result<Stats> {
        let mut stats = self.with(rows::stats)?;
        stats.database_size_bytes = ["", "-wal"]
            .iter()
            .filter_map(|suffix| {
                let mut path = self.path.clone().into_os_string();
                path.push(suffix);
                std::fs::metadata(path).ok()
            })
            .map(|meta| meta.len())
            .sum();
        Ok(stats)
    }

    fn messages(&self, query: &MessageQuery) -> Result<MessagePage> {
        self.with(|conn| {
            let mut filter = self.filter(conn, query.account);
            match query
                .search
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(search) => filter.search(&crate::SearchQuery::parse(search))?,
                None => filter.mailbox(&query.mailbox)?,
            }
            let limit = query.limit.clamp(1, 500);
            let mut messages = filter.page(query.offset, limit + 1)?;
            let has_more = messages.len() as u64 > limit;
            messages.truncate(limit as usize);
            rows::decorate(conn, self.schema, &mut messages)?;
            Ok(MessagePage { messages, has_more })
        })
    }

    fn thread(&self, message: &MessageSummary) -> Result<Thread> {
        self.with(|conn| {
            let ids = match message.conversation_id {
                Some(conversation) => rows::conversation_ids(conn, conversation)?,
                None => vec![message.id],
            };
            let total = ids.len() as u64;
            let anchor = ids.iter().position(|id| *id == message.id).unwrap_or(0);
            let start = anchor.saturating_sub(THREAD_BEFORE);
            let end = (anchor + THREAD_AFTER + 1).min(ids.len());
            let window: Vec<MessageId> = match ids.get(start..end) {
                Some(window) if !window.is_empty() => window.to_vec(),
                _ => vec![message.id],
            };
            let messages = rows::details(conn, self.schema, &window)?;
            if messages.is_empty() {
                return Err(Error::MessageNotFound(message.id.0));
            }
            let subject = messages
                .iter()
                .map(|detail| detail.summary.subject.as_str())
                .find(|subject| !subject.trim().is_empty())
                .unwrap_or_default()
                .to_owned();
            Ok(Thread {
                subject,
                messages,
                total,
            })
        })
    }

    fn attachment(&self, attachment: &Attachment) -> Result<Vec<u8>> {
        self.with(|conn| blobs::read(conn, self.schema, &self.attachments_dir, attachment))
    }
}
