//! msgvault, on Moonframe: a reader for a [msgvault] archive whose whole
//! window (title bar, sidebar, list, reader) is laid out by the Luau in
//! `app/`, while Rust reads the archive through msgvault-core.
//!
//! [msgvault]: https://github.com/wesm/msgvault
//!
//! `cargo run --release` opens it; `-- --check` loads it headlessly; edits
//! to `app/` show live.

mod format;
mod page;
mod scramble;
mod vault;

use {
    moonframe::Moonframe,
    std::{path::Path, process::ExitCode},
};

/// The app's name: its folders are `~/.config/msgvault-moonframe` and
/// `~/.cache/msgvault-moonframe`.
pub(crate) const NAME: &str = "msgvault-moonframe";

/// `app/`, compiled into the binary: what a copy installed anywhere runs.
#[derive(moonframe::rust_embed::RustEmbed)]
#[crate_path = "moonframe::rust_embed"]
#[folder = "app"]
#[exclude = ".DS_Store"]
#[exclude = "*/.DS_Store"]
struct AppFiles;

/// `app/` in this checkout.
const SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/app");

/// Whether this binary runs from this checkout's `target/` (`cargo run`,
/// `just run`): then `app/` is read from disk and edits show live. A copy
/// installed or packaged elsewhere runs the app compiled into it.
fn from_checkout() -> bool {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    match (
        std::env::current_exe().and_then(|exe| exe.canonicalize()),
        target.canonicalize(),
    ) {
        (Ok(exe), Ok(target)) => exe.starts_with(target) && Path::new(SOURCE).is_dir(),
        _ => false,
    }
}

fn main() -> ExitCode {
    let app = Moonframe::new(NAME)
        .title("msgvault")
        .app_id("dev.penso.msgvault-moonframe")
        .version(env!("CARGO_PKG_VERSION"))
        // Check for Updates…, and a daily check in an installed copy: the
        // newest release here, its disk image verified against SHA256SUMS.
        .updates("moonframehq/msgvault")
        .icon(include_bytes!("../app/icon.png"));
    let app = if from_checkout() {
        app.app_dir(SOURCE)
    } else {
        app.embedded_app::<AppFiles>()
    };
    app
        // Send feedback: the `?` that asks a coding agent to change this app.
        .keep(["feedback"])
        .native("vault", vault::Vault::new)
        .run()
}
