//! Builds the `WHERE` clause for message lists, counts and searches.

use {
    super::{Schema, rows},
    crate::{AccountId, Mailbox, MessageSummary, Result, SearchQuery},
    rusqlite::{Connection, params_from_iter, types::Value},
};

/// Live messages only: `deleted_at` marks a dedup loser, never shown.
const LIVE: &str = "m.deleted_at IS NULL";
/// msgvault treats a NULL or empty `message_type` as email (legacy rows).
const EMAIL: &str = "(m.message_type = 'email' OR m.message_type IS NULL OR m.message_type = '')";

const INBOX: &[&str] = &["INBOX"];
const STARRED: &[&str] = &["STARRED", "FLAGGED"];
const UNREAD: &[&str] = &["UNREAD"];
const IMPORTANT: &[&str] = &["IMPORTANT"];
const SENT: &[&str] = &[
    "SENT",
    "SENT ITEMS",
    "SENT MAIL",
    "SENT MESSAGES",
    "[GMAIL]/SENT MAIL",
];
const DRAFTS: &[&str] = &["DRAFT", "DRAFTS", "[GMAIL]/DRAFTS"];
const SPAM: &[&str] = &["SPAM", "JUNK", "JUNK E-MAIL", "JUNK EMAIL", "[GMAIL]/SPAM"];
const TRASH: &[&str] = &[
    "TRASH",
    "BIN",
    "DELETED ITEMS",
    "DELETED MESSAGES",
    "[GMAIL]/TRASH",
    "[GMAIL]/BIN",
];

pub(crate) struct Filter<'c> {
    conn: &'c Connection,
    schema: Schema,
    account: Option<AccountId>,
    clauses: Vec<String>,
    params: Vec<Value>,
}

impl<'c> Filter<'c> {
    pub(crate) fn new(conn: &'c Connection, schema: Schema, account: Option<AccountId>) -> Self {
        let mut filter = Self {
            conn,
            schema,
            account,
            clauses: vec![LIVE.to_owned()],
            params: Vec::new(),
        };
        if let Some(account) = account {
            filter.clauses.push("m.source_id = ?".into());
            filter.params.push(Value::Integer(account.0));
        }
        filter
    }

    pub(crate) fn mailbox(&mut self, mailbox: &Mailbox) -> Result<()> {
        let clause = match mailbox {
            Mailbox::Inbox => self.labels_named(INBOX)?,
            Mailbox::Starred => {
                let labels = self.labels_named(STARRED)?;
                self.or_imap_flag(labels, "Flagged")
            },
            Mailbox::Unread => {
                let labels = self.labels_named(UNREAD)?;
                if self.schema.imap_flags {
                    format!(
                        "({labels} OR (EXISTS (SELECT 1 FROM imap_message_memberships im \
                         WHERE im.message_id = m.id) AND NOT EXISTS (SELECT 1 FROM \
                         imap_message_memberships im WHERE im.message_id = m.id AND \
                         im.flags LIKE '%Seen%')))"
                    )
                } else {
                    labels
                }
            },
            Mailbox::Important => self.labels_named(IMPORTANT)?,
            Mailbox::Sent => {
                let labels = self.label_clause(self.sent_label_ids()?);
                format!("({labels} OR (m.is_from_me = 1 AND {EMAIL}))")
            },
            Mailbox::Drafts => self.labels_named(DRAFTS)?,
            Mailbox::AllMail => EMAIL.to_owned(),
            Mailbox::Attachments => format!("({EMAIL} AND m.has_attachments = 1)"),
            Mailbox::Spam => self.labels_named(SPAM)?,
            Mailbox::Trash => self.labels_named(TRASH)?,
            Mailbox::RemovedUpstream => "m.deleted_from_source_at IS NOT NULL".into(),
            Mailbox::Label(name) => {
                let ids = self.label_ids("name = ?", vec![Value::Text(name.clone())])?;
                self.label_clause(ids)
            },
            Mailbox::Kind(kind) => {
                self.params.push(Value::Text(kind.clone()));
                "m.message_type = ?".into()
            },
        };
        self.clauses.push(clause);
        Ok(())
    }

    pub(crate) fn search(&mut self, query: &SearchQuery) -> Result<()> {
        if let Some(expression) = query.fts_expression() {
            if self.schema.fts {
                self.clauses.push(
                    "m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?)".into(),
                );
                self.params.push(Value::Text(expression));
            } else {
                for term in &query.text {
                    let like = like(term);
                    self.clauses.push(
                        "(m.subject LIKE ? ESCAPE '\\' OR m.snippet LIKE ? ESCAPE '\\')".into(),
                    );
                    self.params
                        .extend([Value::Text(like.clone()), Value::Text(like)]);
                }
            }
        }
        for from in &query.from {
            let like = like(from);
            self.clauses.push(
                "(EXISTS (SELECT 1 FROM message_recipients mr JOIN participants p \
                 ON p.id = mr.participant_id WHERE mr.message_id = m.id AND \
                 mr.recipient_type = 'from' AND (p.email_address LIKE ?1 ESCAPE '\\' OR \
                 p.display_name LIKE ?1 ESCAPE '\\' OR mr.display_name LIKE ?1 ESCAPE '\\' OR \
                 p.phone_number LIKE ?1 ESCAPE '\\')) OR EXISTS (SELECT 1 FROM participants p \
                 WHERE p.id = m.sender_id AND (p.email_address LIKE ?1 ESCAPE '\\' OR \
                 p.display_name LIKE ?1 ESCAPE '\\' OR p.phone_number LIKE ?1 ESCAPE '\\')))"
                    .replace("?1", "?"),
            );
            self.params
                .extend(std::iter::repeat_n(Value::Text(like), 7));
        }
        for to in &query.to {
            let like = like(to);
            self.clauses.push(
                "EXISTS (SELECT 1 FROM message_recipients mr JOIN participants p \
                 ON p.id = mr.participant_id WHERE mr.message_id = m.id AND \
                 mr.recipient_type IN ('to', 'cc', 'bcc') AND (p.email_address LIKE ? ESCAPE '\\' \
                 OR p.display_name LIKE ? ESCAPE '\\' OR mr.display_name LIKE ? ESCAPE '\\'))"
                    .into(),
            );
            self.params
                .extend(std::iter::repeat_n(Value::Text(like), 3));
        }
        for subject in &query.subject {
            self.clauses.push("m.subject LIKE ? ESCAPE '\\'".into());
            self.params.push(Value::Text(like(subject)));
        }
        for label in &query.labels {
            let ids = self.label_ids(
                "(name LIKE ? ESCAPE '\\' OR name LIKE ? ESCAPE '\\')",
                vec![
                    Value::Text(escape(label)),
                    Value::Text(format!("CATEGORY_{}", escape(label))),
                ],
            )?;
            let clause = self.label_clause(ids);
            self.clauses.push(clause);
        }
        if query.has_attachment {
            self.clauses.push("m.has_attachments = 1".into());
        }
        if query.unread {
            self.mailbox(&Mailbox::Unread)?;
        }
        if query.starred {
            self.mailbox(&Mailbox::Starred)?;
        }
        // julianday() compares instants correctly across stored UTC offsets.
        if let Some(after) = query.after {
            self.clauses
                .push("julianday(m.sent_at) >= julianday(?)".into());
            self.params
                .push(Value::Text(after.format("%Y-%m-%d").to_string()));
        }
        if let Some(before) = query.before {
            self.clauses
                .push("julianday(m.sent_at) < julianday(?)".into());
            self.params
                .push(Value::Text(before.format("%Y-%m-%d").to_string()));
        }
        Ok(())
    }

    pub(crate) fn count(&self) -> Result<u64> {
        let sql = format!("SELECT COUNT(*) FROM messages m WHERE {}", self.where_sql());
        let count: i64 =
            self.conn
                .query_row(&sql, params_from_iter(self.params.iter()), |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    pub(crate) fn page(&self, offset: u64, limit: u64) -> Result<Vec<MessageSummary>> {
        let sql = format!(
            "{} WHERE {} ORDER BY m.sent_at DESC, m.id DESC LIMIT ? OFFSET ?",
            rows::SUMMARY_SELECT,
            self.where_sql()
        );
        let params = self
            .params
            .iter()
            .cloned()
            .chain([Value::Integer(limit as i64), Value::Integer(offset as i64)]);
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(params), rows::summary)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn where_sql(&self) -> String {
        self.clauses.join(" AND ")
    }

    fn labels_named(&self, names: &[&str]) -> Result<String> {
        let placeholders = vec!["?"; names.len()].join(", ");
        let ids = self.label_ids(
            &format!("UPPER(name) IN ({placeholders})"),
            names
                .iter()
                .map(|name| Value::Text((*name).to_owned()))
                .collect(),
        )?;
        Ok(self.label_clause(ids))
    }

    fn sent_label_ids(&self) -> Result<Vec<i64>> {
        let placeholders = vec!["?"; SENT.len()].join(", ");
        let role = if self.schema.label_system_role {
            " OR system_role = 'sent'"
        } else {
            ""
        };
        self.label_ids(
            &format!("(UPPER(name) IN ({placeholders}){role})"),
            SENT.iter()
                .map(|name| Value::Text((*name).to_owned()))
                .collect(),
        )
    }

    /// Resolves matching labels up front: the labels table is tiny, and an
    /// `IN (ids)` probe on `message_labels`' primary key keeps lists fast.
    fn label_ids(&self, predicate: &str, mut params: Vec<Value>) -> Result<Vec<i64>> {
        let mut sql = format!("SELECT id FROM labels WHERE {predicate}");
        if let Some(account) = self.account {
            sql.push_str(" AND (source_id = ? OR source_id IS NULL)");
            params.push(Value::Integer(account.0));
        }
        let mut statement = self.conn.prepare(&sql)?;
        let ids = statement.query_map(params_from_iter(params), |row| row.get(0))?;
        Ok(ids.collect::<rusqlite::Result<_>>()?)
    }

    fn label_clause(&self, ids: Vec<i64>) -> String {
        if ids.is_empty() {
            return "0".into();
        }
        let ids: Vec<String> = ids.iter().map(i64::to_string).collect();
        format!(
            "EXISTS (SELECT 1 FROM message_labels ml WHERE ml.message_id = m.id AND ml.label_id IN ({}))",
            ids.join(", ")
        )
    }

    fn or_imap_flag(&self, labels: String, flag: &str) -> String {
        if !self.schema.imap_flags {
            return labels;
        }
        format!(
            "({labels} OR EXISTS (SELECT 1 FROM imap_message_memberships im \
             WHERE im.message_id = m.id AND im.flags LIKE '%{flag}%'))"
        )
    }
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn like(value: &str) -> String {
    format!("%{}%", escape(value))
}
