//! Exercises the daemon client against a canned HTTP server speaking the
//! msgvault REST shapes (`api/openapi.yaml`).

use {
    msgvault_core::{
        ArchiveLocation, Backend, BackendChoice, BackendKind, Error, HttpBackend, Mailbox,
        MessageQuery, Result,
    },
    std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    },
};

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn respond(path: &str) -> (u16, String) {
    let body = match path.split('?').next().unwrap_or_default() {
        "/api/ping" => r#"{"ok":true,"pid":1,"service":"msgvault","version":"test"}"#,
        "/api/v1/cli/accounts" => {
            r#"{"accounts":[{"id":1,"email":"alex@example.com","type":"gmail","display_name":"Alex","message_count":2,"last_sync":"2024-05-01T10:00:00Z"}]}"#
        },
        "/api/v1/messages/filter" => {
            r#"{"count":1,"has_more":true,"offset":0,"limit":50,"messages":[{"id":10,"source_id":1,"conversation_id":5,"subject":"Hello","from":"Ada <ada@x.test>","from_email":"ada@x.test","from_name":"Ada","to":["alex@example.com"],"sent_at":"2024-05-01T10:00:00Z","snippet":"Hi","labels":["INBOX","UNREAD","Work"],"has_attachments":false,"size_bytes":100}]}"#
        },
        "/api/v1/search" => r#"{"query":"q","total":1,"page":1,"page_size":50,"messages":[]}"#,
        "/api/v1/conversations/5" => {
            r#"{"id":5,"anchor_id":10,"has_before":false,"has_after":false,"total":2,"messages":[
                {"id":9,"conversation_id":5,"subject":"Hello","from":"Me <alex@example.com>","to":["Ada <ada@x.test>"],"sent_at":"2024-04-30T10:00:00Z","snippet":"","labels":["SENT"],"has_attachments":false,"size_bytes":1,"body":"First","is_from_me":true,"attachments":[]},
                {"id":10,"conversation_id":5,"subject":"Hello","from":"Ada <ada@x.test>","to":["alex@example.com"],"sent_at":"2024-05-01T10:00:00Z","snippet":"","labels":["INBOX"],"has_attachments":true,"size_bytes":1,"body":"","body_omitted":true,"attachments":[]}]}"#
        },
        "/api/v1/messages/10" => {
            r#"{"id":10,"conversation_id":5,"subject":"Hello","from":"Ada <ada@x.test>","to":["alex@example.com"],"cc":["Bo <bo@x.test>"],"sent_at":"2024-05-01T10:00:00Z","snippet":"","labels":["INBOX"],"has_attachments":true,"size_bytes":1,"body":"Full body","body_html":"<p>Full</p>","attachments":[{"id":3,"filename":"a.txt","mime_type":"text/plain","size_bytes":5,"content_hash":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"}]}"#
        },
        "/api/v1/attachments/2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824/content" => {
            "hello"
        },
        _ => return (404, r#"{"error":"not_found"}"#.into()),
    };
    (200, body.to_owned())
}

fn serve() -> std::io::Result<Server> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).map_or(true, |n| n == 0) || header == "\r\n" {
                    break;
                }
            }
            if let Ok(mut log) = log.lock() {
                log.push(path.clone());
            }
            let (status, body) = respond(&path);
            let mut stream = &stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(Server { url, requests })
}

fn server() -> Result<Server> {
    serve().map_err(|source| Error::Io {
        path: "mock server".into(),
        source,
    })
}

#[test]
fn reads_accounts_messages_threads_and_attachments() -> Result<()> {
    let server = server()?;
    let backend = HttpBackend::connect(&server.url, None)?;
    assert_eq!(backend.describe().kind, BackendKind::Daemon);

    let accounts = backend.accounts()?;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].identifier, "alex@example.com");

    let page = backend.messages(&MessageQuery {
        mailbox: Mailbox::Inbox,
        limit: 50,
        ..MessageQuery::default()
    })?;
    assert!(page.has_more);
    let message = &page.messages[0];
    assert!(message.unread && message.from.name == "Ada");

    let thread = backend.thread(message)?;
    assert_eq!(thread.total, 2);
    assert_eq!(thread.messages.len(), 2);
    // The omitted body was fetched individually.
    assert_eq!(thread.messages[1].body_text.as_deref(), Some("Full body"));
    assert_eq!(thread.messages[1].cc[0].address, "bo@x.test");
    assert!(thread.messages[0].is_from_me);

    let attachment = &thread.messages[1].attachments[0];
    assert_eq!(backend.attachment(attachment)?, b"hello");

    let requests = server
        .requests
        .lock()
        .map(|r| r.clone())
        .unwrap_or_default();
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("/api/v1/messages/filter?label=INBOX"))
    );
    assert!(requests.iter().any(|r| r.contains("anchor=10")));
    Ok(())
}

#[test]
fn discovery_reads_runtime_records() -> Result<()> {
    let server = server()?;
    let dir = tempfile::tempdir().map_err(|e| Error::Io {
        path: "tempdir".into(),
        source: e,
    })?;
    let port = server.url.rsplit(':').next().unwrap_or_default();
    std::fs::write(
        dir.path().join("daemon.99.json"),
        format!(r#"{{"pid":99,"metadata":{{"host":"127.0.0.1","port":"{port}"}}}}"#),
    )
    .map_err(|e| Error::Io {
        path: "record".into(),
        source: e,
    })?;
    let location = ArchiveLocation::load(dir.path())?;
    let backend = msgvault_core::open(&location, &BackendChoice::Auto)?;
    assert_eq!(backend.describe().location, server.url);
    Ok(())
}

#[test]
fn unreachable_daemon_is_typed() {
    let result = HttpBackend::connect("http://127.0.0.1:9", None);
    assert!(matches!(result, Err(Error::Http { .. })));
}
