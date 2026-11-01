//! Row mapping and the batch queries shared by lists and conversations.

use {
    super::{Schema, raw},
    crate::{
        Account, AccountId, Address, Attachment, AttachmentId, ConversationId, Label,
        MessageDetail, MessageId, MessageSummary, Result, Stats,
        time::{from_epoch, parse_timestamp},
    },
    chrono::{DateTime, Utc},
    rusqlite::{Connection, Row, params_from_iter, types::ValueRef},
    std::collections::HashMap,
};

/// Summary columns, sender resolution as in msgvault's `ListMessages`
/// (`internal/query/sqlite.go`): the first `from` recipient, else `sender_id`.
pub(crate) const SUMMARY_SELECT: &str = "SELECT m.id, m.conversation_id, m.source_id,
    COALESCE(NULLIF(TRIM(m.subject), ''), conv.title, ''),
    COALESCE(NULLIF(TRIM(m.snippet), ''), SUBSTR(mb.body_text, 1, 240), ''),
    COALESCE(NULLIF(TRIM(mr_from.display_name), ''), NULLIF(TRIM(p.display_name), ''), ''),
    COALESCE(NULLIF(p.email_address, ''), NULLIF(p.phone_number, ''), ''),
    COALESCE(m.sent_at, m.received_at, m.internal_date),
    COALESCE(m.has_attachments, 0), COALESCE(m.message_type, ''),
    conv.message_count, COALESCE(m.size_estimate, 0),
    m.deleted_from_source_at IS NOT NULL, COALESCE(m.is_from_me, 0)
FROM messages m
LEFT JOIN message_recipients mr_from ON mr_from.id = (
    SELECT mr.id FROM message_recipients mr
    WHERE mr.message_id = m.id AND mr.recipient_type = 'from'
    ORDER BY mr.id LIMIT 1)
LEFT JOIN participants p ON p.id = COALESCE(mr_from.participant_id, m.sender_id)
LEFT JOIN conversations conv ON conv.id = m.conversation_id
LEFT JOIN message_bodies mb ON mb.message_id = m.id";

pub(crate) fn summary(row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    Ok(MessageSummary {
        id: MessageId(row.get(0)?),
        conversation_id: row.get::<_, Option<i64>>(1)?.map(ConversationId),
        account_id: row.get::<_, Option<i64>>(2)?.map(AccountId),
        subject: row.get(3)?,
        snippet: clean_snippet(&row.get::<_, String>(4)?),
        from: Address {
            name: row.get(5)?,
            address: row.get(6)?,
        },
        sent_at: timestamp(row, 7)?,
        labels: Vec::new(),
        has_attachments: row.get::<_, i64>(8)? != 0,
        unread: false,
        starred: false,
        message_type: row.get(9)?,
        thread_count: row.get::<_, Option<i64>>(10)?.map(|n| n.max(0) as u64),
        size_bytes: row.get::<_, i64>(11)?.max(0) as u64,
        removed_upstream: row.get(12)?,
    })
}

/// Reads a timestamp column stored as text or as epoch seconds/millis.
pub(crate) fn timestamp(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    Ok(match row.get_ref(index)? {
        ValueRef::Text(text) => std::str::from_utf8(text).ok().and_then(parse_timestamp),
        ValueRef::Integer(value) => from_epoch(value),
        ValueRef::Real(value) => from_epoch(value as i64),
        ValueRef::Null | ValueRef::Blob(_) => None,
    })
}

/// Snippets from Gmail contain HTML entities and runs of whitespace.
fn clean_snippet(raw: &str) -> String {
    let decoded = raw
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

pub(crate) fn accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut statement = conn.prepare(
        "SELECT s.id, s.source_type, s.identifier, s.display_name, s.last_sync_at,
            (SELECT COUNT(*) FROM messages m WHERE m.source_id = s.id AND m.deleted_at IS NULL)
         FROM sources s ORDER BY s.source_type, s.identifier COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Account {
            id: AccountId(row.get(0)?),
            kind: row.get(1)?,
            identifier: row.get(2)?,
            display_name: row.get(3)?,
            last_sync: timestamp(row, 4)?,
            message_count: row.get::<_, i64>(5)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn labels(conn: &Connection, account: Option<AccountId>) -> Result<Vec<Label>> {
    let scope = if account.is_some() {
        "WHERE l.source_id = ?1"
    } else {
        ""
    };
    let sql = format!(
        "SELECT l.name, MAX(l.label_type), MAX(l.color), COUNT(DISTINCT ml.message_id)
         FROM labels l LEFT JOIN message_labels ml ON ml.label_id = l.id
         {scope} GROUP BY l.name ORDER BY l.name COLLATE NOCASE"
    );
    let mut statement = conn.prepare(&sql)?;
    let params: Vec<i64> = account.map(|a| a.0).into_iter().collect();
    let rows = statement.query_map(params_from_iter(params), |row| {
        let name: String = row.get(0)?;
        let label_type: Option<String> = row.get(1)?;
        Ok(Label {
            kind: Label::classify(&name, label_type.as_deref()),
            name,
            color: row
                .get::<_, Option<String>>(2)?
                .filter(|c| !c.trim().is_empty()),
            count: row.get::<_, i64>(3)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Live message counts per `message_type`, with legacy empty types as email.
pub(crate) fn kinds(conn: &Connection, account: Option<AccountId>) -> Result<Vec<(String, u64)>> {
    let scope = if account.is_some() {
        "AND source_id = ?1"
    } else {
        ""
    };
    let sql = format!(
        "SELECT COALESCE(NULLIF(message_type, ''), 'email') AS kind, COUNT(*)
         FROM messages WHERE deleted_at IS NULL {scope} GROUP BY kind ORDER BY 2 DESC"
    );
    let mut statement = conn.prepare(&sql)?;
    let params: Vec<i64> = account.map(|a| a.0).into_iter().collect();
    let rows = statement.query_map(params_from_iter(params), |row| {
        Ok((row.get(0)?, row.get::<_, i64>(1)?.max(0) as u64))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn stats(conn: &Connection) -> Result<Stats> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM messages WHERE deleted_at IS NULL),
            (SELECT COUNT(*) FROM conversations), (SELECT COUNT(*) FROM sources),
            (SELECT COUNT(DISTINCT name) FROM labels), (SELECT COUNT(*) FROM attachments)",
        [],
        |row| {
            let n = |i: usize| row.get::<_, i64>(i).map(|v| v.max(0) as u64);
            Ok(Stats {
                total_messages: n(0)?,
                total_threads: n(1)?,
                total_accounts: n(2)?,
                total_labels: n(3)?,
                total_attachments: n(4)?,
                database_size_bytes: 0,
            })
        },
    )?)
}

/// Fills labels and read/starred state for a page of summaries. Gmail keeps
/// these as UNREAD/STARRED labels; IMAP keeps flags on folder memberships.
pub(crate) fn decorate(
    conn: &Connection,
    schema: Schema,
    messages: &mut [MessageSummary],
) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = messages.iter().map(|m| m.id.0).collect();
    let mut labels: HashMap<i64, Vec<String>> = HashMap::new();
    let sql = format!(
        "SELECT ml.message_id, l.name FROM message_labels ml JOIN labels l ON l.id = ml.label_id
         WHERE ml.message_id IN ({}) ORDER BY l.name COLLATE NOCASE",
        placeholders(ids.len())
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(&ids), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, name) = row?;
        let names = labels.entry(id).or_default();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let mut flags: HashMap<i64, (bool, bool)> = HashMap::new();
    if schema.imap_flags {
        let sql = format!(
            "SELECT message_id, MAX(flags LIKE '%Seen%'), MAX(flags LIKE '%Flagged%')
             FROM imap_message_memberships WHERE message_id IN ({}) GROUP BY message_id",
            placeholders(ids.len())
        );
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(&ids), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)? != 0,
                row.get::<_, i64>(2)? != 0,
            ))
        })?;
        for row in rows {
            let (id, seen, flagged) = row?;
            flags.insert(id, (seen, flagged));
        }
    }
    for message in messages.iter_mut() {
        message.labels = labels.remove(&message.id.0).unwrap_or_default();
        let has = |name: &str| message.labels.iter().any(|l| l.eq_ignore_ascii_case(name));
        let (unread, starred) = (has("UNREAD"), has("STARRED"));
        let (imap_unread, imap_starred) = flags
            .get(&message.id.0)
            .map_or((false, false), |(seen, flagged)| (!seen, *flagged));
        message.unread = unread || imap_unread;
        message.starred = starred || imap_starred;
    }
    Ok(())
}

/// Conversation message ids in chronological order, as msgvault's
/// `GetConversationWindow` orders them.
pub(crate) fn conversation_ids(
    conn: &Connection,
    conversation: ConversationId,
) -> Result<Vec<MessageId>> {
    let mut statement = conn.prepare(
        "SELECT id FROM messages WHERE conversation_id = ?1 AND deleted_at IS NULL
         ORDER BY COALESCE(sent_at, received_at, internal_date) ASC, id ASC",
    )?;
    let rows = statement.query_map([conversation.0], |row| row.get(0).map(MessageId))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Full messages for `ids`, returned in the order given.
pub(crate) fn details(
    conn: &Connection,
    schema: Schema,
    ids: &[MessageId],
) -> Result<Vec<MessageDetail>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let raw_ids: Vec<i64> = ids.iter().map(|id| id.0).collect();
    let marks = placeholders(raw_ids.len());

    let sql = format!("{SUMMARY_SELECT} WHERE m.id IN ({marks})");
    let mut statement = conn.prepare(&sql)?;
    let mut from_me: HashMap<i64, bool> = HashMap::new();
    let rows = statement.query_map(params_from_iter(&raw_ids), |row| {
        let summary = summary(row)?;
        Ok((summary, row.get::<_, i64>(13)? != 0))
    })?;
    let mut summaries = Vec::with_capacity(raw_ids.len());
    for row in rows {
        let (summary, is_from_me) = row?;
        from_me.insert(summary.id.0, is_from_me);
        summaries.push(summary);
    }
    decorate(conn, schema, &mut summaries)?;

    let mut recipients: HashMap<i64, [Vec<Address>; 3]> = HashMap::new();
    let sql = format!(
        "SELECT mr.message_id, mr.recipient_type,
            COALESCE(NULLIF(TRIM(mr.display_name), ''), NULLIF(TRIM(p.display_name), ''), ''),
            COALESCE(NULLIF(p.email_address, ''), NULLIF(p.phone_number, ''), '')
         FROM message_recipients mr JOIN participants p ON p.id = mr.participant_id
         WHERE mr.message_id IN ({marks}) AND mr.recipient_type IN ('to', 'cc', 'bcc')
         ORDER BY mr.message_id, mr.id"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(&raw_ids), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, Address {
            name: row.get(2)?,
            address: row.get(3)?,
        }))
    })?;
    for row in rows {
        let (id, kind, address) = row?;
        let slot = match kind.as_str() {
            "to" => 0,
            "cc" => 1,
            _ => 2,
        };
        if let Some(list) = recipients.entry(id).or_default().get_mut(slot) {
            list.push(address);
        }
    }

    let mut bodies: HashMap<i64, (Option<String>, Option<String>)> = HashMap::new();
    let sql = format!(
        "SELECT message_id, body_text, body_html FROM message_bodies WHERE message_id IN ({marks})"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(&raw_ids), |row| {
        Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?))
    })?;
    for row in rows {
        let (id, text, html): (i64, Option<String>, Option<String>) = row?;
        bodies.insert(id, (non_empty(text), non_empty(html)));
    }

    let mut attachments: HashMap<i64, Vec<Attachment>> = HashMap::new();
    let role = if schema.attachment_role {
        "attachment_role"
    } else {
        "NULL"
    };
    let content_id = if schema.attachment_content_id {
        "content_id"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT id, message_id, COALESCE(filename, ''), COALESCE(mime_type, ''), COALESCE(size, 0),
            content_hash, storage_path, {role}, {content_id}
         FROM attachments WHERE message_id IN ({marks}) ORDER BY id"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(&raw_ids), |row| {
        let role: Option<String> = row.get(7)?;
        let content_id: Option<String> = row.get(8)?;
        let inline = matches!(
            role.as_deref(),
            Some("inline" | "avatar" | "thumbnail" | "preview" | "ui_asset")
        ) || content_id
            .as_deref()
            .is_some_and(|cid| cid.starts_with("remote-image:"));
        Ok((row.get::<_, i64>(1)?, Attachment {
            id: AttachmentId(row.get(0)?),
            filename: row.get(2)?,
            mime_type: row.get(3)?,
            size_bytes: row.get::<_, i64>(4)?.max(0) as u64,
            content_hash: row
                .get::<_, Option<String>>(5)?
                .filter(|h| !h.is_empty())
                .map(|h| h.to_ascii_lowercase()),
            storage_path: row.get::<_, Option<String>>(6)?.filter(|p| !p.is_empty()),
            inline,
        }))
    })?;
    for row in rows {
        let (id, attachment) = row?;
        attachments.entry(id).or_default().push(attachment);
    }

    let mut by_id: HashMap<i64, MessageSummary> =
        summaries.into_iter().map(|s| (s.id.0, s)).collect();
    let mut details = Vec::with_capacity(ids.len());
    for id in &raw_ids {
        let Some(summary) = by_id.remove(id) else {
            continue;
        };
        let (mut body_text, mut body_html) = bodies.remove(id).unwrap_or_default();
        if body_text.is_none() && body_html.is_none() {
            // msgvault falls back to the raw MIME when no parsed body exists.
            match raw::bodies(conn, summary.id) {
                Ok(Some((text, html))) => (body_text, body_html) = (text, html),
                Ok(None) => {},
                Err(error) => tracing::warn!(?error, id, "unable to parse raw message"),
            }
        }
        let [to, cc, bcc] = recipients.remove(id).unwrap_or_default();
        details.push(MessageDetail {
            is_from_me: from_me.get(id).copied().unwrap_or(false),
            summary,
            to,
            cc,
            bcc,
            body_text,
            body_html,
            attachments: attachments.remove(id).unwrap_or_default(),
        });
    }
    Ok(details)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}
