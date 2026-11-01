//! Client for a running `msgvault serve` daemon (`api/openapi.yaml`).
//!
//! A local daemon listens on loopback, by default on a random port recorded in
//! `<data_dir>/daemon.<pid>.json`. Without an `[server] api_key` loopback
//! requests need no credentials; msgvault only checks the `Host` header.

use {
    crate::{
        Account, AccountId, Address, ArchiveLocation, Attachment, AttachmentId, Backend,
        BackendInfo, BackendKind, ConversationId, Error, Label, Mailbox, MailboxCounts,
        MessageDetail, MessageId, MessagePage, MessageQuery, MessageSummary, Result, Stats, Thread,
        sqlite::{ATTACHMENT_LIMIT, content_hash, hex},
        time::parse_timestamp,
    },
    serde::{Deserialize, de::DeserializeOwned},
    sha2::{Digest, Sha256},
    std::{collections::HashMap, path::Path, sync::Mutex, time::Duration},
};

const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const THREAD_BEFORE: u32 = 40;
const THREAD_AFTER: u32 = 10;

pub struct HttpBackend {
    base: String,
    api_key: Option<String>,
    agent: ureq::Agent,
    /// Account identifiers by id; search scopes accounts by identifier.
    accounts: Mutex<HashMap<i64, String>>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

impl HttpBackend {
    /// Finds a reachable daemon for this archive: a configured remote, a
    /// fixed `api_port`, then every `daemon.*.json` runtime record.
    pub fn discover(location: &ArchiveLocation) -> Result<Self> {
        let mut candidates: Vec<(String, Option<String>)> = Vec::new();
        if let Some(url) = &location.remote_url {
            candidates.push((url.clone(), location.remote_api_key.clone()));
        }
        if let Some(port) = location.api_port {
            candidates.push((format!("http://127.0.0.1:{port}"), location.api_key.clone()));
        }
        for url in runtime_urls(&location.data_dir) {
            candidates.push((url, location.api_key.clone()));
        }
        candidates
            .into_iter()
            .find_map(|(url, key)| Self::connect(&url, key).ok())
            .ok_or(Error::DaemonNotFound)
    }

    /// Connects to `url` and checks it answers `/api/ping`.
    pub fn connect(url: &str, api_key: Option<String>) -> Result<Self> {
        let base = url.trim_end_matches('/').to_owned();
        let probe = Self {
            base: base.clone(),
            api_key: api_key.clone(),
            agent: agent(PROBE_TIMEOUT),
            accounts: Mutex::default(),
        };
        let ping: Ping = probe.get("/api/ping", &[])?;
        if !ping.ok {
            return Err(Error::DaemonNotFound);
        }
        tracing::info!(%base, version = ping.version, "connected to msgvault daemon");
        Ok(Self {
            agent: agent(REQUEST_TIMEOUT),
            ..probe
        })
    }

    fn request(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let url = format!("{}{path}", self.base);
        let mut request = self.agent.get(&url);
        for (key, value) in query {
            request = request.query(*key, value);
        }
        if let Some(key) = &self.api_key {
            request = request.header("X-API-Key", key);
        }
        request.call().map_err(|error| Error::http(url, error))
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let url = format!("{}{path}", self.base);
        self.request(path, query)?
            .body_mut()
            .read_json()
            .map_err(|error| Error::http(url, error))
    }

    fn account_identifier(&self, account: AccountId) -> Result<Option<String>> {
        let cached = self
            .accounts
            .lock()
            .map(|accounts| accounts.get(&account.0).cloned())
            .unwrap_or_default();
        if cached.is_some() {
            return Ok(cached);
        }
        Ok(self
            .accounts()?
            .into_iter()
            .find(|a| a.id == account)
            .map(|a| a.identifier))
    }

    fn search_total(&self, q: &str, account: Option<AccountId>) -> Result<u64> {
        let mut query = vec![("q", q.to_owned()), ("page_size", "1".to_owned())];
        if let Some(account) = account
            .map(|a| self.account_identifier(a))
            .transpose()?
            .flatten()
        {
            query.push(("account", account));
        }
        let response: SearchResponse = self.get("/api/v1/search", &query)?;
        Ok(response.total.max(0) as u64)
    }

    fn message(&self, id: MessageId) -> Result<MessageDetail> {
        let detail: ApiDetail = self.get(&format!("/api/v1/messages/{id}"), &[])?;
        Ok(detail.into())
    }
}

/// Daemon URLs from msgvault's runtime records. The record format belongs to
/// msgvault, so read only the fields needed and tolerate the rest.
fn runtime_urls(data_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("daemon.") && name.ends_with(".json")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let metadata = &record["metadata"];
        let port = metadata["port"]
            .as_str()
            .and_then(|p| p.parse::<u16>().ok());
        let host = metadata["host"]
            .as_str()
            .filter(|h| !h.is_empty() && *h != "0.0.0.0" && *h != "::")
            .unwrap_or("127.0.0.1");
        if let Some(port) = port {
            urls.push(format!("http://{}:{port}", bracket_ipv6(host)));
            continue;
        }
        let address = record["address"]
            .as_str()
            .or_else(|| record["address"]["address"].as_str())
            .or_else(|| record["endpoint"]["address"].as_str());
        if let Some(address) = address.filter(|a| a.contains(':')) {
            urls.push(format!("http://{address}"));
        }
    }
    urls
}

fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

#[derive(Deserialize)]
struct Ping {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize)]
struct AccountsResponse {
    #[serde(default)]
    accounts: Vec<ApiAccount>,
}

#[derive(Deserialize)]
struct ApiAccount {
    id: i64,
    #[serde(default)]
    email: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    message_count: i64,
    #[serde(default)]
    last_sync: Option<String>,
}

#[derive(Deserialize)]
struct AggregateResponse {
    #[serde(default)]
    rows: Vec<AggregateRow>,
}

#[derive(Deserialize)]
struct AggregateRow {
    key: String,
    #[serde(default)]
    count: i64,
}

#[derive(Deserialize)]
struct StatsResponse {
    #[serde(default)]
    total_messages: i64,
    #[serde(default)]
    total_threads: i64,
    #[serde(default)]
    total_accounts: i64,
    #[serde(default)]
    total_labels: i64,
    #[serde(default)]
    total_attachments: i64,
    #[serde(default)]
    database_size_bytes: i64,
}

#[derive(Deserialize)]
struct FilteredResponse {
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    messages: Vec<ApiSummary>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    total: i64,
    #[serde(default)]
    messages: Vec<ApiSummary>,
}

#[derive(Deserialize)]
struct ConversationResponse {
    #[serde(default)]
    messages: Vec<ApiDetail>,
    #[serde(default)]
    total: i64,
}

#[derive(Deserialize)]
struct ApiSummary {
    id: i64,
    #[serde(default)]
    source_id: Option<i64>,
    #[serde(default)]
    conversation_id: Option<i64>,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    from_email: String,
    #[serde(default)]
    from_name: String,
    #[serde(default)]
    from_phone: String,
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    sent_at: String,
    #[serde(default)]
    deleted_at: String,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    has_attachments: bool,
    #[serde(default)]
    size_bytes: i64,
}

#[derive(Deserialize)]
struct ApiDetail {
    #[serde(flatten)]
    summary: ApiSummary,
    #[serde(default)]
    body: String,
    #[serde(default)]
    body_html: String,
    #[serde(default)]
    is_from_me: bool,
    #[serde(default)]
    body_omitted: bool,
    #[serde(default)]
    attachments: Vec<ApiAttachment>,
}

#[derive(Deserialize)]
struct ApiAttachment {
    id: i64,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    size_bytes: i64,
    #[serde(default)]
    content_hash: String,
    #[serde(default)]
    url: String,
}

/// Parses `Name <addr>` as msgvault formats participants.
fn parse_address(raw: &str) -> Address {
    let raw = raw.trim();
    match raw.rsplit_once('<') {
        Some((name, rest)) if rest.ends_with('>') => Address {
            name: name.trim().trim_matches('"').to_owned(),
            address: rest.trim_end_matches('>').trim().to_owned(),
        },
        _ => Address {
            name: String::new(),
            address: raw.to_owned(),
        },
    }
}

impl From<ApiSummary> for MessageSummary {
    fn from(api: ApiSummary) -> Self {
        let has = |name: &str| api.labels.iter().any(|l| l.eq_ignore_ascii_case(name));
        let (unread, starred) = (has("UNREAD"), has("STARRED"));
        let address = [api.from_email.as_str(), api.from_phone.as_str()]
            .into_iter()
            .find(|a| !a.is_empty())
            .map(str::to_owned);
        let from = match address {
            Some(address) => Address {
                name: api.from_name,
                address,
            },
            None => parse_address(&api.from),
        };
        Self {
            id: MessageId(api.id),
            conversation_id: api.conversation_id.filter(|id| *id > 0).map(ConversationId),
            account_id: api.source_id.filter(|id| *id > 0).map(AccountId),
            subject: api.subject,
            snippet: api.snippet,
            from,
            sent_at: parse_timestamp(&api.sent_at),
            labels: api.labels,
            has_attachments: api.has_attachments,
            unread,
            starred,
            message_type: api.message_type,
            thread_count: None,
            size_bytes: api.size_bytes.max(0) as u64,
            removed_upstream: !api.deleted_at.is_empty(),
        }
    }
}

impl From<ApiDetail> for MessageDetail {
    fn from(api: ApiDetail) -> Self {
        let addresses = |list: &[String]| list.iter().map(|a| parse_address(a)).collect();
        let (to, cc, bcc) = (
            addresses(&api.summary.to),
            addresses(&api.summary.cc),
            addresses(&api.summary.bcc),
        );
        let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
        Self {
            to,
            cc,
            bcc,
            body_text: non_empty(api.body),
            body_html: non_empty(api.body_html),
            is_from_me: api.is_from_me,
            attachments: api
                .attachments
                .into_iter()
                .map(|a| Attachment {
                    id: AttachmentId(a.id),
                    filename: a.filename,
                    mime_type: a.mime_type,
                    size_bytes: a.size_bytes.max(0) as u64,
                    content_hash: non_empty(a.content_hash).map(|h| h.to_ascii_lowercase()),
                    storage_path: non_empty(a.url),
                    inline: false,
                })
                .collect(),
            summary: api.summary.into(),
        }
    }
}

fn mailbox_params(mailbox: &Mailbox) -> Option<Vec<(&'static str, String)>> {
    let label = |name: &str| Some(vec![("label", name.to_owned())]);
    match mailbox {
        Mailbox::Inbox => label("INBOX"),
        Mailbox::Starred => label("STARRED"),
        Mailbox::Unread => label("UNREAD"),
        Mailbox::Important => label("IMPORTANT"),
        Mailbox::Sent => label("SENT"),
        Mailbox::Drafts => label("DRAFT"),
        Mailbox::Spam => label("SPAM"),
        Mailbox::Trash => label("TRASH"),
        Mailbox::Label(name) => label(name),
        Mailbox::AllMail => Some(vec![("message_type", "email".to_owned())]),
        Mailbox::Attachments => Some(vec![("attachments_only", "true".to_owned())]),
        Mailbox::Kind(kind) => Some(vec![("message_type", kind.clone())]),
        // The filter endpoint cannot select only source-deleted messages.
        Mailbox::RemovedUpstream => None,
    }
}

impl Backend for HttpBackend {
    fn describe(&self) -> BackendInfo {
        BackendInfo {
            kind: BackendKind::Daemon,
            location: self.base.clone(),
        }
    }

    fn accounts(&self) -> Result<Vec<Account>> {
        let response: AccountsResponse = self.get("/api/v1/cli/accounts", &[])?;
        let accounts: Vec<Account> = response
            .accounts
            .into_iter()
            .map(|a| Account {
                id: AccountId(a.id),
                kind: a.kind,
                identifier: a.email,
                display_name: Some(a.display_name).filter(|n| !n.is_empty()),
                message_count: a.message_count.max(0) as u64,
                last_sync: a.last_sync.as_deref().and_then(parse_timestamp),
            })
            .collect();
        if let Ok(mut cache) = self.accounts.lock() {
            *cache = accounts
                .iter()
                .map(|a| (a.id.0, a.identifier.clone()))
                .collect();
        }
        Ok(accounts)
    }

    fn labels(&self, account: Option<AccountId>) -> Result<Vec<Label>> {
        let mut query = vec![
            ("view_type", "labels".to_owned()),
            ("limit", "1000".to_owned()),
            ("sort", "name".to_owned()),
            ("direction", "asc".to_owned()),
        ];
        if let Some(account) = account {
            query.push(("source_id", account.0.to_string()));
        }
        let response: AggregateResponse = self.get("/api/v1/aggregates", &query)?;
        let mut labels: Vec<Label> = response
            .rows
            .into_iter()
            .map(|row| Label {
                kind: Label::classify(&row.key, None),
                name: row.key,
                color: None,
                count: row.count.max(0) as u64,
            })
            .collect();
        labels.sort_by_key(|label| label.name.to_lowercase());
        Ok(labels)
    }

    fn mailbox_counts(&self, account: Option<AccountId>) -> Result<MailboxCounts> {
        let labels = self.labels(account)?;
        let count = |name: &str| {
            labels
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(name))
                .map_or(0, |l| l.count)
        };
        let all_mail = match account {
            Some(id) => self
                .accounts()?
                .iter()
                .find(|a| a.id == id)
                .map_or(0, |a| a.message_count),
            None => self.stats()?.total_messages,
        };
        Ok(MailboxCounts {
            inbox_unread: self.search_total("label:INBOX label:UNREAD", account)?,
            inbox: count("INBOX"),
            starred: count("STARRED"),
            drafts: count("DRAFT"),
            all_mail,
            attachments: self.search_total("has:attachment", account)?,
            removed_upstream: 0,
            kinds: Vec::new(),
        })
    }

    fn stats(&self) -> Result<Stats> {
        let s: StatsResponse = self.get("/api/v1/stats", &[])?;
        let n = |v: i64| v.max(0) as u64;
        Ok(Stats {
            total_messages: n(s.total_messages),
            total_threads: n(s.total_threads),
            total_accounts: n(s.total_accounts),
            total_labels: n(s.total_labels),
            total_attachments: n(s.total_attachments),
            database_size_bytes: n(s.database_size_bytes),
        })
    }

    fn messages(&self, query: &MessageQuery) -> Result<MessagePage> {
        let limit = query.limit.clamp(1, 100);
        if let Some(search) = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut params = vec![
                ("q", search.to_owned()),
                ("page", (query.offset / limit + 1).to_string()),
                ("page_size", limit.to_string()),
            ];
            if let Some(account) = query
                .account
                .map(|a| self.account_identifier(a))
                .transpose()?
                .flatten()
            {
                params.push(("account", account));
            }
            let response: SearchResponse = self.get("/api/v1/search", &params)?;
            let seen = query.offset + response.messages.len() as u64;
            return Ok(MessagePage {
                has_more: (seen as i64) < response.total,
                messages: response.messages.into_iter().map(Into::into).collect(),
            });
        }
        let Some(mut params) = mailbox_params(&query.mailbox) else {
            return Ok(MessagePage::default());
        };
        params.extend([
            ("offset", query.offset.to_string()),
            ("limit", limit.to_string()),
            ("sort", "date".to_owned()),
            ("direction", "desc".to_owned()),
        ]);
        if let Some(account) = query.account {
            params.push(("source_id", account.0.to_string()));
        }
        let response: FilteredResponse = self.get("/api/v1/messages/filter", &params)?;
        Ok(MessagePage {
            has_more: response.has_more,
            messages: response.messages.into_iter().map(Into::into).collect(),
        })
    }

    fn thread(&self, message: &MessageSummary) -> Result<Thread> {
        let Some(conversation) = message.conversation_id else {
            let detail = self.message(message.id)?;
            return Ok(Thread {
                subject: detail.summary.subject.clone(),
                messages: vec![detail],
                total: 1,
            });
        };
        let response: ConversationResponse =
            self.get(&format!("/api/v1/conversations/{conversation}"), &[
                ("anchor", message.id.to_string()),
                ("before", THREAD_BEFORE.to_string()),
                ("after", THREAD_AFTER.to_string()),
            ])?;
        let total = response.total.max(0) as u64;
        let mut messages = Vec::with_capacity(response.messages.len());
        for api in response.messages {
            // The window drops bodies past its size budget; fetch those singly.
            if api.body_omitted {
                messages.push(self.message(MessageId(api.summary.id))?);
            } else {
                messages.push(api.into());
            }
        }
        let subject = messages
            .iter()
            .map(|m: &MessageDetail| m.summary.subject.as_str())
            .find(|s| !s.trim().is_empty())
            .unwrap_or(&message.subject)
            .to_owned();
        Ok(Thread {
            subject,
            messages,
            total,
        })
    }

    fn attachment(&self, attachment: &Attachment) -> Result<Vec<u8>> {
        let hash = content_hash(attachment).ok_or(Error::AttachmentMissing)?;
        let path = format!("/api/v1/attachments/{hash}/content");
        let url = format!("{}{path}", self.base);
        let bytes = self
            .request(&path, &[])?
            .body_mut()
            .with_config()
            .limit(ATTACHMENT_LIMIT)
            .read_to_vec()
            .map_err(|error| Error::http(url, error))?;
        let actual = hex(&Sha256::digest(&bytes));
        if actual != hash {
            return Err(Error::AttachmentCorrupt {
                expected: hash,
                actual,
            });
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_from_api_strings() {
        assert_eq!(parse_address("\"Ada L\" <ada@example.com>"), Address {
            name: "Ada L".into(),
            address: "ada@example.com".into()
        });
        assert_eq!(parse_address("bob@example.com").address, "bob@example.com");
    }

    #[test]
    fn runtime_records_yield_loopback_urls() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("daemon.4242.json"),
            r#"{"pid":4242,"address":{"network":"tcp","address":"127.0.0.1:51234"},
               "metadata":{"host":"127.0.0.1","port":"51234"}}"#,
        )?;
        std::fs::write(
            dir.path().join("daemon.1.json"),
            r#"{"address":"localhost:7000"}"#,
        )?;
        std::fs::write(dir.path().join("config.toml"), "")?;
        let mut urls = runtime_urls(dir.path());
        urls.sort();
        assert_eq!(urls, vec![
            "http://127.0.0.1:51234",
            "http://localhost:7000"
        ]);
        Ok(())
    }

    #[test]
    fn summaries_derive_state_from_labels() -> serde_json::Result<()> {
        let api: ApiSummary = serde_json::from_str(
            r#"{"id":7,"conversation_id":3,"subject":"Hi","from":"Ada <ada@x.test>",
                "to":["b@x.test"],"sent_at":"2024-05-01T10:00:00Z","snippet":"s",
                "labels":["INBOX","UNREAD"],"has_attachments":true,"size_bytes":10}"#,
        )?;
        let summary = MessageSummary::from(api);
        assert!(summary.unread && !summary.starred && summary.has_attachments);
        assert_eq!(summary.from.address, "ada@x.test");
        assert_eq!(summary.conversation_id, Some(ConversationId(3)));
        assert!(summary.sent_at.is_some());
        Ok(())
    }
}
