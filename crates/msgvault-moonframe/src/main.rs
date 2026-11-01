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
mod vault;

use {moonframe::Moonframe, std::process::ExitCode};

/// The app's name: its folders are `~/.config/msgvault-moonframe` and
/// `~/.cache/msgvault-moonframe`.
pub(crate) const NAME: &str = "msgvault-moonframe";

fn main() -> ExitCode {
    Moonframe::new(NAME)
        .title("msgvault")
        .app_id("dev.penso.msgvault-moonframe")
        .version(env!("CARGO_PKG_VERSION"))
        .icon(include_bytes!("../app/icon.png"))
        .app_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/app"))
        // Send feedback: the `?` that asks a coding agent to change this app.
        .keep(["feedback"])
        .native("vault", vault::Vault::new)
        .run()
}
