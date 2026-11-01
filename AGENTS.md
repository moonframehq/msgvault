# AGENTS.md

Guidance for agents working in this repository.

## Architecture

msgvault-moonframe is a read-only reader for msgvault archives on the
Moonframe library. msgvault (Go) owns the data: never write to a user's
archive.

| Location | Responsibility |
| --- | --- |
| `crates/msgvault-core` | From msgvault-gpui: archive location/config, model, the `Backend` trait (`SqliteBackend`, `HttpBackend`), fixtures. Keep it free of UI code. |
| `crates/msgvault-moonframe/src` | The app on `moonframe`: `vault.rs` (the native provider, `app.vault`), `page.rs` (HTML mail pages), `format.rs` (dates, sizes, colours, file kinds). |
| `crates/msgvault-moonframe/app` | The UI, all Luau: `main.luau` (the shell's layout and menus), `mods/*`, `components/*`, `style.luau`. |

- **Layout and look live in Luau.** Change `app/` for anything on screen; it
  reloads live. Add to `vault.rs` only what Luau can't do: reading the
  archive, files, opening and saving.
- **The provider's contract:** what `Vault::snapshot` publishes is what the
  mods read as `app.vault`; the messages `Vault::handle` takes are what they
  send to `provider:vault`. Change both sides together.
- Only the `reader` and `list` mods may open or save attachments, and only on
  a click.
- **Attachments** are content-addressed: msgvault-core verifies the SHA-256.
  Treat filenames as untrusted (`format::safe_filename`).
- HTML pages run no script and load nothing remote unless the user asked for
  that message (`page::write`).

- **Speed.** `Vault::snapshot` reads each part through `Cache` (accounts
  and totals, counts and labels by account, the list by mailbox, account and
  search, the thread by message). Invalidate only what a message changes; a
  sync (`Vault::synced`, the database files' size and time) clears it all.
  Check the `vault: <click> in N ms (...)` log line after a change.

## Rust conventions

- No `unwrap`/`expect` in production code; the workspace lints deny them
  (tests use `#[allow(clippy::unwrap_used, clippy::expect_used)]`). No
  `unsafe`.
- Keep dependency versions in the root `[workspace.dependencies]`. Moonframe
  is pinned by `rev`, with only the features this app uses (`acp` for Send
  feedback, `alacritty`, `tokio`): `git`, `docs` and `herdr-core` would pull
  in the herdr-gpui crates for nothing.

## Releases

`just release` runs `.github/workflows/release.yml` on `main`'s HEAD, as
Moonframe does; see docs/releasing.md. A release binary runs the app compiled
into it (`AppFiles`); only one running from this checkout's `target/` reads
`app/` live. Keep workflow actions pinned by SHA and check them with
`just audit`.

## Verification

- `just format`, then `just ci`: formatting, lockfile, clippy with
  `-D warnings`, tests, and `--check` (the app loaded headlessly on the demo
  archive).
- When dependencies change, also run `just deny`.
- For UI changes, run `just demo` and look at the window. Use the Moonframe
  skills (`moonframe`, `moonframe-components`) for the Luau.
