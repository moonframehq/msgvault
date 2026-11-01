//! `vault`, the native provider every view reads: the archive through
//! msgvault-core, shared as `app.vault`, shaped for the Luau views in
//! `app/`.
//!
//! It opens the archive the way msgvault-gpui does: a running `msgvault
//! serve` daemon first, else `msgvault.db` read-only, at `$MSGVAULT_HOME`
//! (else `~/.msgvault`). Without one (or with `MSGVAULT_DEMO=1`) it opens
//! msgvault-core's generated demo archive, and the title bar says "Sample".
//!
//! Mods change what is shown by message: `{ mailbox = "inbox" |
//! "label:Work" | "kind:whatsapp" | "files" | ... }`, `{ account = id }` (0
//! for all), `{ query = "text" }` (msgvault's search syntax), `{ open =
//! message id }`, `{ remote = message id }` (load its remote pictures),
//! `{ refresh = true }`, and in the attachments browser `{ file = id }`,
//! `{ kind = "image" | ... }`, `{ fquery = true, value = "text" }` and
//! `{ show = message id }`. Only the reader and the list may ask for
//! `{ open_file = id }` (a temporary copy, opened in the system's app) and
//! `{ save_file = id }` (into Downloads), each on a click. What is shown is
//! kept in `~/.cache/msgvault-moonframe/view.json`.

use {
    crate::{format, page, scramble::Mask},
    chrono::{DateTime, Local, Utc},
    moonframe::{Data, Handle, Native},
    msgvault_core::{
        self as mv, Account, AccountId, ArchiveLocation, Attachment, Backend, BackendChoice,
        BackendKind, LabelKind, Mailbox, MessageDetail, MessageQuery, MessageSummary,
    },
    serde_json::{Value, json},
    std::{
        collections::{BTreeSet, HashMap, HashSet},
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Arc,
    },
};

/// Rows published for one list.
const ROWS: u64 = 400;
/// Messages with attachments read for the attachments browser.
const FILE_MESSAGES: u64 = 200;
/// The largest picture shown inline.
const THUMB_LIMIT: u64 = 8 * 1024 * 1024;

/// What the user looks at, kept across launches.
#[derive(Clone, Debug, PartialEq)]
struct View {
    mailbox: String,
    account: i64,
    query: String,
    open: i64,
    fkind: String,
    fquery: String,
    file: String,
    /// Messages whose remote pictures the user let load.
    remote: BTreeSet<i64>,
    /// Made-up names and text in place of the archive's, for screenshots.
    scramble: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            mailbox: "inbox".into(),
            account: 0,
            query: String::new(),
            open: 0,
            fkind: "all".into(),
            fquery: String::new(),
            file: String::new(),
            remote: BTreeSet::new(),
            scramble: false,
        }
    }
}

impl View {
    fn read(path: &Path) -> Self {
        let Some(saved) = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            return Self::default();
        };
        let text = |key: &str, default: &str| {
            saved
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(default)
                .to_owned()
        };
        let number = |key: &str| saved.get(key).and_then(Value::as_i64).unwrap_or(0);
        Self {
            mailbox: text("mailbox", "inbox"),
            account: number("account"),
            query: text("query", ""),
            open: number("open"),
            fkind: text("fkind", "all"),
            fquery: text("fquery", ""),
            file: text("file", ""),
            remote: saved
                .get("remote")
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default(),
            scramble: saved
                .get("scramble")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    fn save(&self, path: &Path) {
        let value = json!({
            "mailbox": self.mailbox, "account": self.account, "query": self.query,
            "open": self.open, "fkind": self.fkind, "fquery": self.fquery,
            "file": self.file, "remote": self.remote, "scramble": self.scramble,
        });
        let partial = path.with_extension("json.tmp");
        let written = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&partial, value.to_string()))
            .and_then(|()| fs::rename(&partial, path));
        if let Err(error) = written {
            tracing::warn!(%error, "vault: cannot keep the view");
        }
    }

    /// The mailbox to list; `None` for the attachments browser.
    fn mailbox(&self) -> Option<Mailbox> {
        Some(match self.mailbox.as_str() {
            "inbox" => Mailbox::Inbox,
            "starred" => Mailbox::Starred,
            "unread" => Mailbox::Unread,
            "important" => Mailbox::Important,
            "sent" => Mailbox::Sent,
            "drafts" => Mailbox::Drafts,
            "attachments" => Mailbox::Attachments,
            "spam" => Mailbox::Spam,
            "trash" => Mailbox::Trash,
            "removed" => Mailbox::RemovedUpstream,
            "files" => return None,
            other => match (other.strip_prefix("label:"), other.strip_prefix("kind:")) {
                (Some(label), _) => Mailbox::Label(label.to_owned()),
                (_, Some(kind)) => Mailbox::Kind(kind.to_owned()),
                _ => Mailbox::AllMail,
            },
        })
    }
}

/// An attachment and the message it came in.
#[derive(Clone, Debug)]
struct File {
    attachment: Attachment,
    message: MessageSummary,
}

impl File {
    fn id(&self) -> String {
        format!("a{}", self.attachment.id.0)
    }

    fn kind(&self) -> &'static str {
        format::file_kind(&self.attachment.mime_type, &self.attachment.filename)
    }
}

/// The archive, once opened.
struct Archive {
    backend: Arc<dyn Backend>,
    sample: bool,
}

pub(crate) struct Vault {
    archive: Result<Archive, String>,
    view: View,
    /// Where the view, the HTML pages and the pictures go.
    cache: PathBuf,
    /// Messages seen in a list or a thread, by id: what `open` needs.
    known: HashMap<i64, MessageSummary>,
    /// Attachments seen, by id: what open, save and the browser need.
    files: HashMap<String, File>,
    /// The attachments browser's files, newest first, once read.
    browser: Option<Vec<String>>,
    thumb_failed: HashSet<String>,
    /// While scrambled, labels go out under made-up names: the real one
    /// for each, to read the views' messages.
    real_labels: HashMap<String, String>,
    /// This install's secret for scrambling.
    key: crate::scramble::Key,
    notice: Option<Value>,
    notices: i64,
}

impl Vault {
    pub(crate) fn new() -> Self {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::NAME);
        // Beside the cache, never in the views: what scrambling is keyed by.
        let key = crate::scramble::key(&cache.join("scramble.key"));
        Self {
            archive: Err("Not opened yet".into()),
            view: View::read(&cache.join("view.json")),
            cache,
            known: HashMap::new(),
            files: HashMap::new(),
            browser: None,
            thumb_failed: HashSet::new(),
            real_labels: HashMap::new(),
            key,
            notice: None,
            notices: 0,
        }
    }

    fn open(&mut self) {
        self.known.clear();
        self.files.clear();
        self.browser = None;
        let demo = std::env::var("MSGVAULT_DEMO").is_ok_and(|value| value == "1");
        let real = (!demo)
            .then(|| {
                ArchiveLocation::load(ArchiveLocation::default_home())
                    .and_then(|location| mv::open(&location, &BackendChoice::Auto))
            })
            .and_then(|opened| {
                opened
                    .inspect_err(|error| tracing::info!(%error, "vault: no archive; sample data"))
                    .ok()
            });
        self.archive = match real {
            Some(backend) => Ok(Archive {
                backend,
                sample: false,
            }),
            None => {
                let dir = self.cache.join("demo");
                mv::fixture::generate(&dir, Utc::now())
                    .and_then(|_| ArchiveLocation::load(&dir))
                    .and_then(|location| mv::open(&location, &BackendChoice::Sqlite))
                    .map(|backend| Archive {
                        backend,
                        sample: true,
                    })
                    .map_err(|error| format!("No msgvault archive, and no demo one: {error}"))
            },
        };
    }

    fn mask(&self) -> Mask {
        Mask {
            on: self.view.scramble,
            key: self.key,
        }
    }

    fn tell(&mut self, ok: bool, text: impl Into<String>) {
        self.notices += 1;
        self.notice = Some(json!({ "seq": self.notices, "ok": ok, "text": text.into() }));
    }

    fn publish(&mut self, host: &Handle) {
        match self.snapshot() {
            Ok(value) => host.publish_json(&value),
            Err(reason) => host.publish_json(&json!({ "available": false, "reason": reason })),
        }
    }

    fn backend(&self) -> Result<Arc<dyn Backend>, String> {
        self.archive
            .as_ref()
            .map(|archive| Arc::clone(&archive.backend))
            .map_err(Clone::clone)
    }

    /// Everything the views show, as `app.vault`.
    fn snapshot(&mut self) -> Result<Value, String> {
        let backend = self.backend()?;
        let sample = self.archive.as_ref().is_ok_and(|archive| archive.sample);
        let now = Local::now();
        let fail = |error: mv::Error| error.to_string();

        let accounts = backend.accounts().map_err(fail)?;
        if !accounts
            .iter()
            .any(|account| account.id.0 == self.view.account)
        {
            self.view.account = 0;
        }
        let account = (self.view.account != 0).then_some(AccountId(self.view.account));
        let current = accounts.iter().find(|a| Some(a.id) == account);

        let counts = backend.mailbox_counts(account).map_err(fail)?;
        let labels = backend.labels(account).map_err(fail)?;
        let colors: HashMap<&str, Option<&str>> = labels
            .iter()
            .map(|label| (label.name.as_str(), label.color.as_deref()))
            .collect();
        let mask = self.mask();
        self.real_labels = labels
            .iter()
            .filter(|label| mask.on && label.kind == LabelKind::User)
            .map(|label| (mask.label(&label.name), label.name.clone()))
            .collect();
        let entry = |label: &mv::Label| {
            let title = match label.kind {
                LabelKind::User => mask.label(&label.display_name()),
                _ => label.display_name(),
            };
            json!({
                "name": if label.kind == LabelKind::User { mask.label(&label.name) } else { label.name.clone() },
                "title": title, "count": label.count,
                "color": format::label_color(&label.name, label.color.as_deref()),
            })
        };
        let shown = |kind: LabelKind| {
            labels
                .iter()
                .filter(move |label| label.kind == kind && label.count > 0)
                .map(entry)
                .collect::<Vec<_>>()
        };
        let mut kinds: Vec<&(String, u64)> = counts
            .kinds
            .iter()
            .filter(|(kind, count)| !matches!(kind.as_str(), "" | "email") && *count > 0)
            .collect();
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut archive: Vec<Value> = kinds
            .iter()
            .map(|(kind, count)| {
                json!({
                    "id": format!("kind:{kind}"), "title": mv::display_kind(kind),
                    "icon": format::kind_icon(kind), "count": count,
                })
            })
            .collect();
        if counts.removed_upstream > 0 {
            archive.push(json!({
                "id": "removed", "title": "Removed Upstream", "icon": "cloud-off",
                "count": counts.removed_upstream,
            }));
        }

        let header = match current {
            Some(account) => json!({
                "title": mask.name(account.title()),
                "subtitle": format!("{} · {}", account_kind(account), mask.address(&account.identifier)),
                "icon": format::account_icon(&account.kind),
            }),
            None => json!({
                "title": "All Accounts",
                "subtitle": format::plural(accounts.len() as u64, "source", "sources"),
                "icon": "archive",
            }),
        };

        let query = self.view.query.trim().to_owned();
        let list = match self.view.mailbox() {
            None if query.is_empty() => {
                json!({ "title": "All Attachments", "subtitle": "", "rows": [] })
            },
            mailbox => {
                let page = backend
                    .messages(&MessageQuery {
                        mailbox: mailbox.clone().unwrap_or_default(),
                        account,
                        search: (!query.is_empty()).then(|| query.clone()),
                        offset: 0,
                        limit: ROWS,
                    })
                    .map_err(fail)?;
                let rows = by_conversation(page.messages);
                for message in &rows {
                    self.known.insert(message.id.0, message.clone());
                }
                let mut subtitle = if query.is_empty() {
                    format::plural(rows.len() as u64, "conversation", "conversations")
                } else {
                    format!(
                        "{} for “{}”",
                        format::plural(rows.len() as u64, "result", "results"),
                        mask.text(&query)
                    )
                };
                if let Some(account) = current {
                    subtitle = format!("{subtitle} · {}", mask.name(account.title()));
                }
                if page.has_more {
                    subtitle.push('+');
                }
                json!({
                    "title": if query.is_empty() {
                        match mailbox.unwrap_or_default() {
                            Mailbox::Label(name) if mv::Label::classify(&name, None) == LabelKind::User => {
                                mask.label(&mv::display_label(&name))
                            },
                            other => other.title(),
                        }
                    } else {
                        "Search".into()
                    },
                    "subtitle": subtitle,
                    "rows": rows.iter().map(|m| row(m, now, &colors, mask)).collect::<Vec<_>>(),
                })
            },
        };

        let thread = match self.known.get(&self.view.open).cloned() {
            Some(summary) => match backend.thread(&summary) {
                Ok(thread) => Some(self.thread(&thread, &summary, &accounts, &colors)),
                Err(error) => {
                    tracing::warn!(%error, "vault: cannot read the thread");
                    None
                },
            },
            None => None,
        };
        let files =
            (self.view.mailbox == "files" && query.is_empty()).then(|| self.browser(&backend, now));

        let stats = backend.stats().map_err(fail)?;
        let info = backend.describe();
        Ok(json!({
            "available": true,
            "sample": sample,
            "scrambled": mask.on,
            "backend": if sample {
                "Sample"
            } else if info.kind == BackendKind::Daemon {
                "Daemon"
            } else {
                "SQLite"
            },
            "location": if sample {
                "demo archive".to_owned()
            } else if mask.on {
                String::new()
            } else {
                tilde(&info.location)
            },
            "stats": {
                "messages": format::plural(stats.total_messages, "message", "messages"),
                "threads": format::plural(stats.total_threads, "thread", "threads"),
                "attachments": format::plural(stats.total_attachments, "attachment", "attachments"),
                "size": format::bytes(stats.database_size_bytes),
            },
            "header": header,
            "accounts": accounts.iter().map(|account| json!({
                "id": account.id.0, "title": mask.name(account.title()), "kind": account_kind(account),
                "identifier": mask.address(&account.identifier), "icon": format::account_icon(&account.kind),
                "count": account.message_count,
            })).collect::<Vec<_>>(),
            "account": self.view.account,
            "counts": {
                "inbox": counts.inbox_unread, "starred": counts.starred, "drafts": counts.drafts,
                "all": counts.all_mail, "attachments": counts.attachments,
                "removed": counts.removed_upstream,
            },
            "labels": shown(LabelKind::User),
            "categories": shown(LabelKind::Category),
            "archive": archive,
            "mailbox": match self.view.mailbox.strip_prefix("label:") {
                Some(name) if mask.on && mv::Label::classify(name, None) == LabelKind::User => {
                    format!("label:{}", mask.label(name))
                },
                _ => self.view.mailbox.clone(),
            },
            "query": query,
            "list": list,
            "selected": self.view.open,
            "thread": thread,
            "total_files": stats.total_attachments,
            "files": files,
            "notice": self.notice,
        }))
    }

    /// The open conversation, oldest first.
    fn thread(
        &mut self,
        thread: &mv::Thread,
        first: &MessageSummary,
        accounts: &[Account],
        colors: &HashMap<&str, Option<&str>>,
    ) -> Value {
        let mask = self.mask();
        let chat = thread.messages.iter().any(|m| !m.summary.is_email());
        let mut labels: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        for message in &thread.messages {
            for chip in chips(&message.summary, usize::MAX, colors, mask) {
                if seen.insert(chip["title"].as_str().unwrap_or_default().to_owned()) {
                    labels.push(chip);
                }
            }
        }
        let mut meta = vec![format::plural(
            thread.total.max(thread.messages.len() as u64),
            "message",
            "messages",
        )];
        if chat {
            meta.insert(0, mv::display_kind(&first.message_type));
        }
        if let Some(account) = accounts.iter().find(|a| Some(a.id) == first.account_id) {
            let title = mask.name(account.title());
            if !meta.contains(&title) {
                meta.push(title);
            }
        }
        let subject = if thread.subject.trim().is_empty() {
            "(no subject)".to_owned()
        } else {
            mask.text(&thread.subject)
        };
        let messages: Vec<Value> = thread
            .messages
            .iter()
            .map(|message| self.message(message))
            .collect();
        json!({
            "id": first.id.0,
            "subject": subject,
            "chat": chat,
            "meta": meta.join(" · "),
            "labels": labels,
            "messages": messages,
        })
    }

    fn message(&mut self, detail: &MessageDetail) -> Value {
        let summary = &detail.summary;
        self.known.insert(summary.id.0, summary.clone());
        let mask = self.mask();
        let sender = mask.name(&person(summary));
        let initials = format::initials(&sender);
        let body = detail
            .body_text
            .clone()
            .filter(|text| !text.trim().is_empty())
            .or_else(|| detail.body_html.as_deref().map(page::html_to_text))
            .unwrap_or_else(|| summary.snippet.clone());
        let names = |people: &[mv::Address]| {
            people
                .iter()
                .map(|person| mask.name(person.label()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let files: Vec<Value> = detail
            .attachments
            .iter()
            .filter(|attachment| !attachment.inline)
            .map(|attachment| {
                let file = File {
                    attachment: attachment.clone(),
                    message: summary.clone(),
                };
                self.files.insert(file.id(), file.clone());
                self.card(&file, None)
            })
            .collect();
        let html = detail
            .body_html
            .as_deref()
            // Scrambled, mail shows as its made-up text: a page would be real.
            .filter(|_| summary.is_email() && !mask.on)
            .map(|html| self.html(summary.id.0, html));
        json!({
            "id": summary.id.0,
            "from": sender,
            "address": if person(summary) != summary.from.address { mask.address_of(&person(summary), &summary.from.address) } else { String::new() },
            "to": names(&detail.to),
            "cc": names(&detail.cc),
            "date": format::full_date(summary.sent_at),
            "time": summary.sent_at.map(format::clock).unwrap_or_default(),
            "snippet": mask.text(&page::clean(&summary.snippet)),
            "body": mask.text(body.trim_end()),
            "mine": detail.is_from_me,
            "starred": summary.starred,
            "attachments": summary.has_attachments,
            "initials": initials,
            "hue": format::hue(&initials),
            "files": files,
            "html": html,
        })
    }

    /// The rendered form of a message with an HTML part: a page for each
    /// look, with remote pictures only once the user let them load.
    fn html(&self, id: i64, html: &str) -> Value {
        let remote = page::remote_content(html);
        let allowed = remote && self.view.remote.contains(&id);
        let dir = self.cache.join("html");
        let url = |dark| {
            page::write(&dir, id, html, dark, allowed)
                .map(|file| format!("file://{}", file.display()))
                .inspect_err(|error| tracing::warn!(%error, "vault: cannot write a page"))
                .ok()
        };
        json!({
            "light": url(false),
            "dark": url(true),
            "height": page::height(html),
            "remote": remote,
            "allowed": allowed,
        })
    }

    /// The attachments browser: every file, filtered by kind and a search.
    fn browser(&mut self, backend: &Arc<dyn Backend>, now: DateTime<Local>) -> Value {
        if self.browser.is_none() {
            self.browser = Some(self.read_files(backend));
        }
        let account = self.view.account;
        let all: Vec<File> = self
            .browser
            .iter()
            .flatten()
            .filter_map(|id| self.files.get(id))
            .filter(|file| account == 0 || file.message.account_id == Some(AccountId(account)))
            .cloned()
            .collect();
        let mut kinds = vec![json!({ "id": "all", "title": "All", "count": all.len() })];
        for (id, title, _) in format::FILE_KINDS {
            let count = all.iter().filter(|file| file.kind() == id).count();
            if count > 0 {
                kinds.push(json!({ "id": id, "title": title, "count": count }));
            }
        }
        let mut kind = self.view.fkind.clone();
        if !kinds.iter().any(|entry| entry["id"] == kind.as_str()) {
            kind = "all".into();
        }
        let query = self.view.fquery.to_lowercase();
        let rows: Vec<&File> = all
            .iter()
            .filter(|file| kind == "all" || file.kind() == kind)
            .filter(|file| {
                query.is_empty()
                    || format!(
                        "{} {} {}",
                        file.attachment.filename,
                        person(&file.message),
                        file.message.subject
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        let size: u64 = rows.iter().map(|file| file.attachment.size_bytes).sum();
        let more = rows.len() > ROWS as usize;
        let cards: Vec<Value> = rows
            .iter()
            .take(ROWS as usize)
            .map(|file| self.card(file, Some(now)))
            .collect();
        let selected = all.iter().find(|file| file.id() == self.view.file);
        json!({
            "title": "All Attachments",
            "subtitle": format!(
                "{}{} · {}",
                format::plural(cards.len() as u64, "file", "files"),
                if more { "+" } else { "" },
                format::bytes(size)
            ),
            "kinds": kinds,
            "kind": kind,
            "query": self.view.fquery,
            "rows": cards,
            "selected": selected.map(File::id).unwrap_or_default(),
            "file": selected.map(|file| self.card(file, None)),
        })
    }

    /// The newest messages with attachments, read thread by thread: the
    /// archive's files, newest first, by id.
    fn read_files(&mut self, backend: &Arc<dyn Backend>) -> Vec<String> {
        let page = backend.messages(&MessageQuery {
            mailbox: Mailbox::Attachments,
            account: None,
            search: None,
            offset: 0,
            limit: FILE_MESSAGES,
        });
        let mut found: Vec<File> = Vec::new();
        let mut read = HashSet::new();
        for summary in page.map(|page| page.messages).unwrap_or_default() {
            let key = summary.conversation_id.map_or(-summary.id.0, |c| c.0);
            if !read.insert(key) {
                continue;
            }
            let Ok(thread) = backend.thread(&summary) else {
                continue;
            };
            for detail in thread.messages {
                self.known
                    .insert(detail.summary.id.0, detail.summary.clone());
                for attachment in detail.attachments.into_iter().filter(|a| !a.inline) {
                    found.push(File {
                        attachment,
                        message: detail.summary.clone(),
                    });
                }
            }
        }
        found.sort_by(|a, b| {
            b.message
                .sent_at
                .cmp(&a.message.sent_at)
                .then_with(|| a.attachment.filename.cmp(&b.attachment.filename))
        });
        found.dedup_by_key(|file| file.attachment.id);
        found
            .into_iter()
            .map(|file| {
                let id = file.id();
                self.files.insert(id.clone(), file);
                id
            })
            .collect()
    }

    /// A file as the reader and the browser show it; dated for a list row
    /// when given `now`, in full otherwise.
    fn card(&mut self, file: &File, now: Option<DateTime<Local>>) -> Value {
        let kind = file.kind();
        let (_, title, icon) = format::FILE_KINDS
            .iter()
            .find(|(id, ..)| *id == kind)
            .copied()
            .unwrap_or(("other", "Other", "file"));
        let name = if file.attachment.filename.is_empty() {
            "attachment".to_owned()
        } else {
            file.attachment.filename.clone()
        };
        let mask = self.mask();
        json!({
            "id": file.id(),
            "name": mask.file(&name),
            "size": if file.attachment.remote_url().is_some() {
                "Remote file".to_owned()
            } else {
                format::bytes(file.attachment.size_bytes)
            },
            "kind": kind,
            "type": title.strip_suffix('s').unwrap_or(title),
            "icon": icon,
            "thumb": if mask.on { None } else { self.thumbnail(file) },
            "from": mask.name(&person(&file.message)),
            "subject": mask.text(&file.message.subject),
            "message": file.message.id.0,
            "date": match now {
                Some(now) => format::list_date(file.message.sent_at, now),
                None => format::full_date(file.message.sent_at),
            },
        })
    }

    /// A picture attachment as a file in the image cache, for `ui.image`.
    fn thumbnail(&mut self, file: &File) -> Option<String> {
        let attachment = &file.attachment;
        if file.kind() != "image"
            || attachment.size_bytes > THUMB_LIMIT
            || attachment.remote_url().is_some()
        {
            return None;
        }
        let name = attachment.filename.to_ascii_lowercase();
        // Only a known picture extension, never text from the message: a
        // MIME type such as `image/../../x` must not choose the path.
        const PICTURES: [&str; 9] = [
            "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic",
        ];
        let mime = attachment.mime_type.to_ascii_lowercase();
        let ext = name
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .or_else(|| mime.strip_prefix("image/"))
            .and_then(|ext| PICTURES.iter().find(|known| **known == ext))
            .copied()
            .unwrap_or("png");
        let key: String = attachment
            .content_hash
            .clone()
            .unwrap_or_else(|| file.id())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .collect();
        let path = self
            .cache
            .join("images")
            .join("msgvault")
            .join(format!("{key}.{ext}"));
        if path.is_file() {
            return Some(path.display().to_string());
        }
        if self.thumb_failed.contains(&key) {
            return None;
        }
        let written = self
            .backend()
            .ok()
            .and_then(|backend| backend.attachment(attachment).ok())
            .and_then(|bytes| {
                let partial = path.with_extension("tmp");
                path.parent()
                    .map_or(Ok(()), fs::create_dir_all)
                    .and_then(|()| fs::write(&partial, bytes))
                    .and_then(|()| fs::rename(&partial, &path))
                    .ok()
            });
        if written.is_none() {
            self.thumb_failed.insert(key);
            return None;
        }
        Some(path.display().to_string())
    }

    /// Opens (in the system's app for it) or saves (to Downloads, then
    /// shows it in the file manager) an attachment, on the user's click.
    fn deliver(&mut self, id: &str, save: bool) {
        let Some(file) = self.files.get(id).cloned() else {
            return self.tell(false, "That attachment is no longer in the archive");
        };
        let name = format::safe_filename(&file.attachment.filename, "attachment");
        // What the notice says: scrambled, the file's made-up name.
        let mask = self.mask();
        let said = mask.file(&name);
        if let Some(url) = file.attachment.remote_url() {
            system_open(&[url]);
            return self.tell(true, format!("Opened {said} in the browser"));
        }
        let bytes = match self.backend().and_then(|backend| {
            backend
                .attachment(&file.attachment)
                .map_err(|error| error.to_string())
        }) {
            Ok(bytes) => bytes,
            Err(error) => {
                return self.tell(false, format!("Can't open {said}: {}", mask.text(&error)));
            },
        };
        let path = if save {
            let dir = std::env::var_os("MSGVAULT_DOWNLOADS")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join("Downloads")))
                .unwrap_or_else(std::env::temp_dir);
            unique(&dir, &name)
        } else {
            std::env::temp_dir()
                .join(crate::NAME)
                .join(file.id())
                .join(&name)
        };
        let written = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, bytes));
        if let Err(error) = written {
            return self.tell(
                false,
                if mask.on {
                    format!("Can't write {said}: {error}")
                } else {
                    format!(
                        "Can't write {}: {error}",
                        tilde(&path.display().to_string())
                    )
                },
            );
        }
        let shown = path.display().to_string();
        if save {
            if cfg!(target_os = "macos") {
                system_open(&["-R", &shown]);
            }
            let saved = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(name);
            self.tell(true, format!("Saved {} to Downloads", mask.file(&saved)));
        } else {
            quarantine(&path);
            system_open(&[&shown]);
            self.tell(true, format!("Opened {said}"));
        }
    }

    fn handle(&mut self, from: &str, message: &Data) {
        let text = |key: &str| message.get(key).and_then(Data::as_str).map(str::to_owned);
        let number = |key: &str| message.get(key).and_then(Data::as_i64);
        let flag = |key: &str| message.get(key).and_then(Data::as_bool) == Some(true);
        if let Some(mailbox) = text("mailbox") {
            // A made-up label name, while scrambled, stands for a real one.
            self.view.mailbox = match mailbox.strip_prefix("label:") {
                Some(name) => self
                    .real_labels
                    .get(name)
                    .map_or(mailbox.clone(), |real| format!("label:{real}")),
                None => mailbox,
            };
            self.view.query.clear();
        }
        if let Some(account) = number("account") {
            self.view.account = account;
        }
        if let Some(query) = text("query") {
            // A search is over messages: it leaves the attachments browser.
            if !query.trim().is_empty() && self.view.mailbox == "files" {
                self.view.mailbox = "all".into();
            }
            self.view.query = query;
        }
        if let Some(open) = number("open") {
            self.view.open = open;
        }
        if let Some(kind) = text("kind") {
            self.view.fkind = kind;
        }
        if message.get("fquery").is_some() {
            // The browser's search field sends its text as `value`.
            self.view.fquery = text("value").unwrap_or_default();
        }
        if flag("fquery_clear") {
            self.view.fquery.clear();
        }
        if let Some(file) = text("file") {
            self.view.file = file;
        }
        if let Some(show) = number("show") {
            // From the attachments browser to the message a file came in.
            self.view.mailbox = "all".into();
            self.view.query.clear();
            self.view.open = show;
        }
        if let Some(remote) = number("remote").filter(|id| *id != 0) {
            self.view.remote.insert(remote);
        }
        if matches!(from, "reader" | "list") {
            if let Some(id) = text("open_file") {
                self.deliver(&id, false);
            }
            if let Some(id) = text("save_file") {
                self.deliver(&id, true);
            }
        }
        if text("scramble").as_deref() == Some("toggle") {
            self.view.scramble = !self.view.scramble;
        }
        if flag("refresh") {
            self.open();
        }
        self.view.save(&self.cache.join("view.json"));
    }
}

impl Native for Vault {
    fn start(&mut self, host: &Handle) -> Result<(), String> {
        self.open();
        self.publish(host);
        Ok(())
    }

    /// Every `interval` of provider.toml: the archive may have synced.
    fn tick(&mut self, host: &Handle) {
        self.publish(host);
    }

    fn receive(&mut self, host: &Handle, from: &str, message: Data) {
        self.handle(from, &message);
        self.publish(host);
    }
}

/// One row per conversation: its newest message, with the older ones'
/// unread, starred and attachment state folded in, as msgvault-gpui lists.
fn by_conversation(messages: Vec<MessageSummary>) -> Vec<MessageSummary> {
    let mut rows: Vec<MessageSummary> = Vec::new();
    let mut row_of: HashMap<i64, usize> = HashMap::new();
    for message in messages {
        let Some(conversation) = message.conversation_id else {
            rows.push(message);
            continue;
        };
        if let Some(&index) = row_of.get(&conversation.0) {
            let row = &mut rows[index];
            row.unread |= message.unread;
            row.starred |= message.starred;
            row.has_attachments |= message.has_attachments;
            continue;
        }
        row_of.insert(conversation.0, rows.len());
        rows.push(message);
    }
    rows
}

fn person(message: &MessageSummary) -> String {
    match message.from.label().trim() {
        "" => "Unknown".into(),
        name => name.to_owned(),
    }
}

fn account_kind(account: &Account) -> String {
    mv::display_kind(&account.kind)
}

/// Up to `most` label chips: the labels not shown as state.
fn chips(
    message: &MessageSummary,
    most: usize,
    colors: &HashMap<&str, Option<&str>>,
    mask: Mask,
) -> Vec<Value> {
    message
        .visible_labels()
        .take(most)
        .map(|name| {
            let title = mv::display_label(name);
            json!({
                "title": match mv::Label::classify(name, None) {
                    LabelKind::User => mask.label(&title),
                    _ => title,
                },
                "color": format::label_color(name, colors.get(name).copied().flatten()),
            })
        })
        .collect()
}

fn row(
    message: &MessageSummary,
    now: DateTime<Local>,
    colors: &HashMap<&str, Option<&str>>,
    mask: Mask,
) -> Value {
    let sender = mask.name(&person(message));
    let subject = mask.text(message.subject.trim());
    let subject = subject.as_str();
    let (title, subject) = if message.is_email() {
        (
            sender,
            if subject.is_empty() {
                "(no subject)".to_owned()
            } else {
                subject.to_owned()
            },
        )
    } else {
        (
            if subject.is_empty() {
                mv::display_kind(&message.message_type)
            } else {
                subject.to_owned()
            },
            sender,
        )
    };
    let initials = format::initials(&title);
    json!({
        "id": message.id.0,
        "title": title,
        "subject": subject,
        "snippet": mask.text(&page::clean(&message.snippet)),
        "date": format::list_date(message.sent_at, now),
        "unread": message.unread,
        "starred": message.starred,
        "attachments": message.has_attachments,
        "removed": message.removed_upstream,
        "count": match message.thread_count {
            Some(count) if message.is_email() && count > 1 => count,
            _ => 0,
        },
        "chips": chips(message, 2, colors, mask),
        "hue": format::hue(&initials),
        "initials": initials,
    })
}

/// `~/...` for a path under the home folder.
fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        },
        _ => path.to_owned(),
    }
}

/// `dir/name`, or `dir/stem (n).ext` when that is taken.
fn unique(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

/// Marks a file written from an attachment as downloaded (macOS), so the
/// system asks before running anything executable in it.
fn quarantine(path: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let stamp = format!("0081;{:x};{};", Utc::now().timestamp(), crate::NAME);
    let marked = Command::new("xattr")
        .args(["-w", "com.apple.quarantine", &stamp])
        .arg(path)
        .status();
    if !marked.is_ok_and(|status| status.success()) {
        tracing::warn!(path = %path.display(), "vault: cannot mark the file as downloaded");
    }
}

/// The system's opener (`open` on macOS, `xdg-open` elsewhere), unless
/// `MSGVAULT_NO_OPEN` is set (tests, screenshots).
fn system_open(args: &[&str]) {
    if std::env::var_os("MSGVAULT_NO_OPEN").is_some() {
        return;
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let spawned = Command::new(opener)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status();
    if let Err(error) = spawned {
        tracing::warn!(%error, "vault: cannot run {opener}");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use {super::*, moonframe::moonframe_core::Data as D};

    /// A vault on the demo archive, its cache in a temporary folder.
    fn demo() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::new();
        vault.cache = dir.path().to_path_buf();
        vault.view = View::default();
        let data = dir.path().join("demo");
        mv::fixture::generate(&data, Utc::now()).unwrap();
        let location = ArchiveLocation::load(&data).unwrap();
        vault.archive = Ok(Archive {
            backend: mv::open(&location, &BackendChoice::Sqlite).unwrap(),
            sample: true,
        });
        (dir, vault)
    }

    #[test]
    fn publishes_the_inbox_and_opens_a_thread() {
        let (_dir, mut vault) = demo();
        let snapshot = vault.snapshot().unwrap();
        assert_eq!(snapshot["available"], true);
        assert_eq!(snapshot["backend"], "Sample");
        let rows = snapshot["list"]["rows"].as_array().unwrap();
        assert!(!rows.is_empty(), "the demo inbox has mail");
        let id = rows[0]["id"].as_i64().unwrap();

        vault.handle("list", &D::map([("open", D::from(id))]));
        let snapshot = vault.snapshot().unwrap();
        let thread = &snapshot["thread"];
        assert_eq!(thread["id"], id);
        assert!(!thread["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn searches_and_lists_every_attachment() {
        let (_dir, mut vault) = demo();
        vault.handle("titlebar", &D::map([("query", D::from("has:attachment"))]));
        let snapshot = vault.snapshot().unwrap();
        assert_eq!(snapshot["list"]["title"], "Search");

        vault.handle("sidebar", &D::map([("mailbox", D::from("files"))]));
        let snapshot = vault.snapshot().unwrap();
        let files = &snapshot["files"];
        assert!(
            !files["rows"].as_array().unwrap().is_empty(),
            "the demo archive has files"
        );
        assert_eq!(files["kinds"][0]["id"], "all");
    }

    #[test]
    fn only_the_reader_and_list_may_open_files() {
        let (dir, mut vault) = demo();
        vault.handle("sidebar", &D::map([("mailbox", D::from("files"))]));
        let snapshot = vault.snapshot().unwrap();
        let id = snapshot["files"]["rows"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        vault.handle("sidebar", &D::map([("save_file", D::from(id.as_str()))]));
        assert!(
            vault.notice.is_none(),
            "a mod other than reader or list was obeyed"
        );
        let _ = dir;
    }

    #[test]
    fn scrambled_nothing_real_reaches_the_views() {
        let (_dir, mut vault) = demo();
        let plain = vault.snapshot().unwrap();
        let id = plain["list"]["rows"][0]["id"].as_i64().unwrap();
        vault.handle("list", &D::map([("open", D::from(id))]));
        let real = vault.snapshot().unwrap().to_string();
        for known in [
            "Priya",
            "Raman",
            "migration",
            "lumen.dev",
            "Atlas",
            "atlas-roadmap",
        ] {
            assert!(real.contains(known), "the demo archive has {known}");
        }

        vault.handle("titlebar", &D::map([("scramble", D::from("toggle"))]));
        let scrambled = vault.snapshot().unwrap();
        assert_eq!(scrambled["scrambled"], true);
        assert!(scrambled["thread"]["messages"][0]["html"].is_null());
        let text = scrambled.to_string();
        for known in [
            "Priya",
            "Raman",
            "migration",
            "lumen.dev",
            "Atlas",
            "atlas-roadmap",
        ] {
            assert!(!text.contains(known), "{known} shown while scrambled");
        }
        // The same people, scrambled the same way, and kept across launches.
        assert_eq!(text, vault.snapshot().unwrap().to_string());
        assert!(View::read(&vault.cache.join("view.json")).scramble);

        // A made-up label opens the real one.
        let label = scrambled["labels"][0]["name"].as_str().unwrap().to_owned();
        vault.handle(
            "sidebar",
            &D::map([("mailbox", D::from(format!("label:{label}")))]),
        );
        assert!(
            vault
                .real_labels
                .values()
                .any(|real| vault.view.mailbox == format!("label:{real}"))
        );
        let listed = vault.snapshot().unwrap();
        assert_eq!(listed["mailbox"], format!("label:{label}"));
        assert!(!listed["list"]["rows"].as_array().unwrap().is_empty());
    }

    #[test]
    fn folds_a_conversation_into_its_newest_row() {
        let message = |id, conversation, unread| MessageSummary {
            id: mv::MessageId(id),
            conversation_id: Some(mv::ConversationId(conversation)),
            account_id: None,
            subject: String::new(),
            snippet: String::new(),
            from: mv::Address::default(),
            sent_at: None,
            labels: Vec::new(),
            has_attachments: false,
            unread,
            starred: false,
            message_type: String::new(),
            thread_count: None,
            size_bytes: 0,
            removed_upstream: false,
        };
        let rows = by_conversation(vec![
            message(3, 1, false),
            message(2, 1, true),
            message(1, 2, false),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id.0, 3);
        assert!(rows[0].unread);
    }
}
