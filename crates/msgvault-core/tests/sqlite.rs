use msgvault_core::{
    ArchiveLocation, Backend, BackendChoice, BackendKind, Error, LabelKind, Mailbox, MessageQuery,
    Result, SqliteBackend, fixture,
};

fn archive() -> Result<(tempfile::TempDir, SqliteBackend)> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io {
        path: "tempdir".into(),
        source: e,
    })?;
    fixture::generate(dir.path(), chrono::Utc::now())?;
    let backend = SqliteBackend::open(&ArchiveLocation::load(dir.path())?)?;
    Ok((dir, backend))
}

fn query(mailbox: Mailbox) -> MessageQuery {
    MessageQuery {
        mailbox,
        limit: 500,
        ..MessageQuery::default()
    }
}

#[test]
fn lists_accounts_labels_and_counts() -> Result<()> {
    let (_dir, backend) = archive()?;
    assert_eq!(backend.describe().kind, BackendKind::Sqlite);
    let accounts = backend.accounts()?;
    assert_eq!(accounts.len(), 5);
    assert!(accounts.iter().all(|a| a.message_count > 0));

    let labels = backend.labels(None)?;
    let work = labels.iter().find(|l| l.name == "Work");
    assert!(work.is_some_and(|l| l.kind == LabelKind::User && l.count > 0 && l.color.is_some()));
    assert!(labels.iter().any(|l| l.kind == LabelKind::Category));

    let counts = backend.mailbox_counts(None)?;
    assert!(counts.inbox > 0 && counts.inbox_unread > 0 && counts.inbox_unread < counts.inbox);
    assert!(counts.attachments > 0 && counts.all_mail > counts.inbox);
    assert!(
        counts
            .kinds
            .iter()
            .any(|(kind, n)| kind == "whatsapp" && *n == 15)
    );
    assert!(
        counts
            .kinds
            .iter()
            .any(|(kind, _)| kind == "calendar_event")
    );

    let stats = backend.stats()?;
    assert_eq!(stats.total_accounts, 5);
    assert!(stats.total_messages > 100 && stats.database_size_bytes > 0);
    Ok(())
}

#[test]
fn mailboxes_filter_and_page_newest_first() -> Result<()> {
    let (_dir, backend) = archive()?;
    let inbox = backend.messages(&query(Mailbox::Inbox))?;
    assert!(!inbox.messages.is_empty() && !inbox.has_more);
    assert!(inbox.messages.iter().all(|m| m.is_email()));
    assert!(
        inbox
            .messages
            .windows(2)
            .all(|w| w[0].sent_at >= w[1].sent_at)
    );
    // Unread comes from Gmail's UNREAD label and IMAP's missing \Seen flag.
    let unread = backend.messages(&query(Mailbox::Unread))?;
    assert!(unread.messages.iter().all(|m| m.unread));
    assert!(
        unread
            .messages
            .iter()
            .any(|m| m.labels.is_empty() || !m.labels.iter().any(|l| l == "UNREAD"))
    );

    let first = backend.messages(&MessageQuery {
        limit: 10,
        ..query(Mailbox::AllMail)
    })?;
    let second = backend.messages(&MessageQuery {
        limit: 10,
        offset: 10,
        ..query(Mailbox::AllMail)
    })?;
    assert!(first.has_more && first.messages.len() == 10);
    assert!(
        first
            .messages
            .iter()
            .all(|a| second.messages.iter().all(|b| a.id != b.id))
    );

    let sent = backend.messages(&query(Mailbox::Sent))?;
    assert!(!sent.messages.is_empty());
    let starred = backend.messages(&query(Mailbox::Starred))?;
    assert!(starred.messages.iter().all(|m| m.starred));
    let removed = backend.messages(&query(Mailbox::RemovedUpstream))?;
    assert!(removed.messages.iter().all(|m| m.removed_upstream));
    let chats = backend.messages(&query(Mailbox::Kind("whatsapp".into())))?;
    assert_eq!(chats.messages.len(), 15);
    assert!(
        chats
            .messages
            .iter()
            .all(|m| !m.snippet.is_empty() && !m.subject.is_empty())
    );
    let work = backend.messages(&query(Mailbox::Label("Work".into())))?;
    assert!(
        work.messages
            .iter()
            .all(|m| m.labels.iter().any(|l| l == "Work"))
    );

    let account = backend.accounts()?.into_iter().find(|a| a.kind == "imap");
    let scoped = backend.messages(&MessageQuery {
        account: account.as_ref().map(|a| a.id),
        ..query(Mailbox::Inbox)
    })?;
    assert!(!scoped.messages.is_empty());
    assert!(
        scoped
            .messages
            .iter()
            .all(|m| m.account_id == account.as_ref().map(|a| a.id))
    );
    Ok(())
}

#[test]
fn search_uses_fts_and_operators() -> Result<()> {
    let (_dir, backend) = archive()?;
    let search = |q: &str| {
        backend.messages(&MessageQuery {
            search: Some(q.into()),
            limit: 500,
            ..MessageQuery::default()
        })
    };
    let hits = search("roadmap")?;
    assert!(!hits.messages.is_empty());
    assert!(
        hits.messages
            .iter()
            .all(|m| m.subject.to_lowercase().contains("roadmap"))
    );
    assert!(
        !search("migrat")?.messages.is_empty(),
        "prefix matching in bodies"
    );
    let from = search("from:priya@lumen.dev")?;
    assert!(
        from.messages
            .iter()
            .all(|m| m.from.address == "priya@lumen.dev")
    );
    assert!(
        search("has:attachment")?
            .messages
            .iter()
            .all(|m| m.has_attachments)
    );
    assert!(
        search("label:promotions")?
            .messages
            .iter()
            .all(|m| m.labels.iter().any(|l| l == "CATEGORY_PROMOTIONS"))
    );
    assert!(search("zzzznothing")?.messages.is_empty());
    assert!(search(r#""unbalanced"#).is_ok());
    Ok(())
}

#[test]
fn threads_load_bodies_recipients_and_attachments() -> Result<()> {
    let (_dir, backend) = archive()?;
    let inbox = backend.messages(&query(Mailbox::AllMail))?;
    let roadmap = inbox
        .messages
        .iter()
        .find(|m| m.subject == "Re: Q3 roadmap review — notes and next steps")
        .ok_or(Error::MessageNotFound(0))?;
    let thread = backend.thread(roadmap)?;
    assert_eq!(thread.messages.len(), 3);
    assert_eq!(thread.total, 3);
    assert!(
        thread
            .messages
            .windows(2)
            .all(|w| w[0].summary.sent_at <= w[1].summary.sent_at)
    );
    assert!(
        thread
            .messages
            .iter()
            .all(|m| m.body_text.is_some() && !m.to.is_empty())
    );
    assert!(thread.messages.iter().any(|m| m.is_from_me));

    // Every stored attachment, loose or zstd-packed, reads back verified.
    let with_files = backend.messages(&query(Mailbox::Attachments))?;
    let mut read = 0;
    for message in &with_files.messages {
        for detail in backend.thread(message)?.messages {
            for attachment in detail.attachments {
                let bytes = backend.attachment(&attachment)?;
                assert_eq!(bytes.len() as u64, attachment.size_bytes);
                read += 1;
            }
        }
    }
    assert!(read >= 6);
    Ok(())
}

#[test]
fn raw_mime_is_the_body_fallback() -> Result<()> {
    let (_dir, backend) = archive()?;
    let hits = backend.messages(&MessageQuery {
        search: Some("subject:\"Hiking this weekend? (follow-up)\"".into()),
        limit: 50,
        ..MessageQuery::default()
    })?;
    let first = hits.messages.first().ok_or(Error::MessageNotFound(0))?;
    let thread = backend.thread(first)?;
    assert!(thread.messages.iter().all(|m| {
        m.body_text
            .as_deref()
            .is_some_and(|body| !body.trim().is_empty())
    }));
    Ok(())
}

#[test]
fn html_bodies_are_kept() -> Result<()> {
    let (_dir, backend) = archive()?;
    let hits = backend.messages(&MessageQuery {
        search: Some("from:hello@rustweekly.dev".into()),
        limit: 5,
        ..MessageQuery::default()
    })?;
    let first = hits.messages.first().ok_or(Error::MessageNotFound(0))?;
    let thread = backend.thread(first)?;
    assert!(
        thread
            .messages
            .iter()
            .all(|m| m.body_html.as_deref().is_some_and(|h| h.contains("<h1>")))
    );
    Ok(())
}

#[test]
fn missing_archive_is_reported() -> Result<()> {
    let dir = tempfile::tempdir().map_err(|e| Error::Io {
        path: "tempdir".into(),
        source: e,
    })?;
    let location = ArchiveLocation::load(dir.path())?;
    assert!(matches!(
        msgvault_core::open(&location, &BackendChoice::Auto),
        Err(Error::ArchiveNotFound { .. })
    ));
    Ok(())
}

#[test]
fn reader_never_writes() -> Result<()> {
    let (dir, backend) = archive()?;
    drop(backend);
    let path = dir.path().join("msgvault.db");
    let before = std::fs::read(&path).map_err(|e| Error::Io {
        path: path.clone(),
        source: e,
    })?;
    let backend = SqliteBackend::open(&ArchiveLocation::load(dir.path())?)?;
    backend.messages(&query(Mailbox::Inbox))?;
    backend.mailbox_counts(None)?;
    drop(backend);
    let after = std::fs::read(&path).map_err(|e| Error::Io { path, source: e })?;
    assert_eq!(before, after);
    Ok(())
}
