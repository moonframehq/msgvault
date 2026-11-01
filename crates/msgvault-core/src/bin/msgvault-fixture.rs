//! Writes a synthetic msgvault archive: `msgvault-fixture [DIR]`.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("demo-archive"));
    match msgvault_core::fixture::generate(&dir, chrono::Utc::now()) {
        Ok(summary) => {
            println!(
                "Wrote {} messages and {} attachments across {} accounts to {}",
                summary.messages,
                summary.attachments,
                summary.accounts,
                dir.display()
            );
            println!(
                "Open it with: MSGVAULT_HOME={} msgvault-gpui",
                dir.display()
            );
            ExitCode::SUCCESS
        },
        Err(error) => {
            eprintln!("msgvault-fixture: {error}");
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        },
    }
}
