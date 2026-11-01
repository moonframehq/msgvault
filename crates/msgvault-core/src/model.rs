//! Backend-neutral view of a msgvault archive, shaped for a mail client.

use {
    chrono::{DateTime, Utc},
    std::fmt,
};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub i64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(
    /// `messages.id`.
    MessageId
);
id_type!(
    /// `conversations.id`: an email thread or a chat.
    ConversationId
);
id_type!(
    /// `sources.id`: one synced account or import.
    AccountId
);
id_type!(
    /// `attachments.id`.
    AttachmentId
);

/// One archived source, such as a Gmail account or a WhatsApp import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    /// `sources.source_type`, e.g. `gmail`, `imap`, `whatsapp`.
    pub kind: String,
    /// Email address, phone number or provider identifier.
    pub identifier: String,
    pub display_name: Option<String>,
    pub message_count: u64,
    pub last_sync: Option<DateTime<Utc>>,
}

impl Account {
    pub fn title(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.identifier)
    }
}

/// Whether a label is provider-managed or user-created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    /// Gmail's INBOX/SENT/... and similar provider folders.
    System,
    /// Gmail's CATEGORY_* tabs.
    Category,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// The stored label name, used for filtering.
    pub name: String,
    pub kind: LabelKind,
    pub color: Option<String>,
    pub count: u64,
}

impl Label {
    pub fn classify(name: &str, label_type: Option<&str>) -> LabelKind {
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("CATEGORY_") {
            return LabelKind::Category;
        }
        if label_type.is_some_and(|kind| kind.eq_ignore_ascii_case("system"))
            || SYSTEM_LABELS.contains(&upper.as_str())
        {
            return LabelKind::System;
        }
        LabelKind::User
    }

    /// Human-friendly name: `CATEGORY_PROMOTIONS` becomes `Promotions`.
    pub fn display_name(&self) -> String {
        display_label(&self.name)
    }
}

pub(crate) const SYSTEM_LABELS: &[&str] = &[
    "INBOX",
    "SENT",
    "TRASH",
    "SPAM",
    "DRAFT",
    "DRAFTS",
    "UNREAD",
    "STARRED",
    "IMPORTANT",
    "CHAT",
];

pub fn display_label(name: &str) -> String {
    let upper = name.to_ascii_uppercase();
    if let Some(category) = upper.strip_prefix("CATEGORY_") {
        return title_case(category);
    }
    if SYSTEM_LABELS.contains(&upper.as_str()) {
        return title_case(&upper);
    }
    name.to_owned()
}

fn title_case(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A standard view over the archive, independent of provider.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mailbox {
    Inbox,
    Starred,
    Unread,
    Important,
    Sent,
    Drafts,
    /// Every email message.
    #[default]
    AllMail,
    Attachments,
    Spam,
    Trash,
    /// Archived here but deleted from the provider.
    RemovedUpstream,
    /// Messages carrying one label.
    Label(String),
    /// Non-email messages of one `messages.message_type` (chats, calendar, ...).
    Kind(String),
}

impl Mailbox {
    pub fn title(&self) -> String {
        match self {
            Self::Inbox => "Inbox".into(),
            Self::Starred => "Starred".into(),
            Self::Unread => "Unread".into(),
            Self::Important => "Important".into(),
            Self::Sent => "Sent".into(),
            Self::Drafts => "Drafts".into(),
            Self::AllMail => "All Mail".into(),
            Self::Attachments => "Attachments".into(),
            Self::Spam => "Spam".into(),
            Self::Trash => "Trash".into(),
            Self::RemovedUpstream => "Removed Upstream".into(),
            Self::Label(name) => display_label(name),
            Self::Kind(kind) => display_kind(kind),
        }
    }
}

/// Friendly name for a `message_type`.
pub fn display_kind(kind: &str) -> String {
    match kind {
        "" | "email" => "Email",
        "imessage" => "iMessage",
        "sms" => "SMS",
        "mms" => "MMS",
        "rcs" => "RCS",
        "whatsapp" => "WhatsApp",
        "fbmessenger" => "Messenger",
        "teams" => "Teams",
        "slack" => "Slack",
        "discord" => "Discord",
        "beeper" => "Beeper",
        "google_chat" => "Google Chat",
        "calendar_event" => "Calendar",
        "meeting_transcript" => "Meetings",
        "matrix" => "Matrix",
        other => return title_case(&other.replace('_', " ")),
    }
    .to_owned()
}

/// What to list. `search` uses msgvault's Gmail-like syntax and, when set,
/// searches every mailbox (still narrowed to `account`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MessageQuery {
    pub mailbox: Mailbox,
    pub account: Option<AccountId>,
    pub search: Option<String>,
    pub offset: u64,
    pub limit: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Address {
    pub name: String,
    /// Email address, or a phone number/handle for chat participants.
    pub address: String,
}

impl Address {
    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.address
        } else {
            &self.name
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.name.trim(), self.address.trim()) {
            ("", address) => f.write_str(address),
            (name, "") => f.write_str(name),
            (name, address) if name == address => f.write_str(address),
            (name, address) => write!(f, "{name} <{address}>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageSummary {
    pub id: MessageId,
    pub conversation_id: Option<ConversationId>,
    pub account_id: Option<AccountId>,
    pub subject: String,
    pub snippet: String,
    pub from: Address,
    pub sent_at: Option<DateTime<Utc>>,
    pub labels: Vec<String>,
    pub has_attachments: bool,
    pub unread: bool,
    pub starred: bool,
    pub message_type: String,
    /// Messages in the conversation, when the backend knows it cheaply.
    pub thread_count: Option<u64>,
    pub size_bytes: u64,
    pub removed_upstream: bool,
}

impl MessageSummary {
    pub fn is_email(&self) -> bool {
        matches!(self.message_type.as_str(), "" | "email")
    }

    /// User-facing labels: everything except the ones rendered as state.
    pub fn visible_labels(&self) -> impl Iterator<Item = &str> {
        self.labels.iter().map(String::as_str).filter(|label| {
            !matches!(
                label.to_ascii_uppercase().as_str(),
                "UNREAD" | "STARRED" | "INBOX" | "SENT" | "DRAFT" | "IMPORTANT"
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub id: AttachmentId,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    /// Lowercase hex SHA-256 of the content, when stored.
    pub content_hash: Option<String>,
    /// Relative blob path, or an http(s) URL for remote-only content.
    pub storage_path: Option<String>,
    /// Referenced from the HTML body (`cid:`), not a user-facing attachment.
    pub inline: bool,
}

impl Attachment {
    pub fn remote_url(&self) -> Option<&str> {
        self.storage_path
            .as_deref()
            .filter(|path| path.starts_with("http://") || path.starts_with("https://"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageDetail {
    pub summary: MessageSummary,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub is_from_me: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessagePage {
    pub messages: Vec<MessageSummary>,
    pub has_more: bool,
}

/// Unread and total counts for the standard mailboxes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailboxCounts {
    pub inbox_unread: u64,
    pub inbox: u64,
    pub starred: u64,
    pub drafts: u64,
    pub all_mail: u64,
    pub attachments: u64,
    pub removed_upstream: u64,
    /// Non-email message types with their counts, largest first.
    pub kinds: Vec<(String, u64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub total_messages: u64,
    pub total_threads: u64,
    pub total_accounts: u64,
    pub total_labels: u64,
    pub total_attachments: u64,
    pub database_size_bytes: u64,
}

/// A conversation window around one message, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Thread {
    pub subject: String,
    pub messages: Vec<MessageDetail>,
    /// Messages in the conversation, including those outside the window.
    pub total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_classified_and_named() {
        assert_eq!(Label::classify("INBOX", None), LabelKind::System);
        assert_eq!(
            Label::classify("CATEGORY_SOCIAL", Some("system")),
            LabelKind::Category
        );
        assert_eq!(Label::classify("Receipts", Some("user")), LabelKind::User);
        assert_eq!(display_label("CATEGORY_PROMOTIONS"), "Promotions");
        assert_eq!(display_label("STARRED"), "Starred");
        assert_eq!(display_label("Travel/2024"), "Travel/2024");
    }

    #[test]
    fn addresses_render_like_headers() {
        let both = Address {
            name: "Ada".into(),
            address: "ada@example.com".into(),
        };
        assert_eq!(both.to_string(), "Ada <ada@example.com>");
        let bare = Address {
            name: String::new(),
            address: "+15551234".into(),
        };
        assert_eq!(bare.to_string(), "+15551234");
        assert_eq!(bare.label(), "+15551234");
    }
}
