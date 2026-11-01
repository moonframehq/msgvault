# msgvault-moonframe

A native reader for [msgvault](https://github.com/wesm/msgvault) archives
(email, chats, labels and attachments), built on
[Moonframe](https://github.com/penso/moonframe): the whole window is laid out
by Luau, while Rust reads the archive.

It is [msgvault-gpui](https://github.com/penso/msgvault-gpui) rebuilt as a
real app on the Moonframe library, to show what one looks like: the same
`msgvault-core` crate, but no hand-written GPUI views.

![The reader on the demo archive](docs/screenshots/reader.png)

## How it is put together

| Location | What it does |
| --- | --- |
| `crates/msgvault-core` | msgvault-gpui's core, unchanged but for its SQLite version: archive location, model, and the `Backend` trait with a read-only SQLite reader and a `msgvault serve` client. |
| `crates/msgvault-moonframe/src/main.rs` | The app: `Moonframe::new("msgvault-moonframe")`, its app folder, and one native provider, `vault`. |
| `crates/msgvault-moonframe/src/vault.rs` | The `vault` provider: reads the archive through msgvault-core and publishes what the views show as `app.vault`; mods send it messages (`{ mailbox = "inbox" }`, `{ open = id }`, `{ query = "from:ada" }`, ...). |
| `crates/msgvault-moonframe/src/page.rs` | HTML mail as a page for the reader's web view: no script, nothing from the network until asked. |
| `crates/msgvault-moonframe/app/` | The window, in Luau: `mods/titlebar`, `mods/sidebar`, `mods/list` and `mods/reader`, two components and `style.luau`. Edits show live while the app runs. |

The app gets Moonframe's shell for free: the menus, light and dark themes,
`--check`, and the **Send feedback** `?`, which asks a coding agent to change
the app's own Luau.

## What it shows

- **Title bar:** search with msgvault's Gmail-like syntax (free text,
  `from:`, `to:`, `subject:`, `label:`, `has:attachment`, `is:unread`,
  `is:starred`, `before:`, `after:`), where the archive is read from,
  refresh (⌘R) and the light/dark switch.
- **Sidebar:** the account in view, the standard mailboxes with counts,
  coloured labels, Gmail's categories, the other sources (WhatsApp, Slack,
  the calendar, messages removed upstream), every account, and the
  archive's size.
- **List:** one row per conversation, its newest message.
- **Reader:** an email thread with earlier messages folded, or a chat as
  bubbles. HTML mail renders in a web view (toggle with ⌘⇧H) with remote
  pictures blocked until you load them. Attachments can be opened or saved,
  only on a click. **All Attachments** lists every file, by kind and search.

- **Scramble** (View › Scramble Names and Text, ⌘⇧X), to share a
  screenshot of a real archive: every name, address, subject, body, label
  and filename is swapped for a made-up one of the same shape (a person keeps
  one name everywhere), HTML mail shows as its scrambled text, pictures are
  hidden, and the title bar says "Scrambled". Counts, dates and layout stay.
  It is kept across launches; ⌘⇧X or the pill turns it off.

## Run it

```sh
just run          # a `msgvault serve` daemon, else $MSGVAULT_HOME or ~/.msgvault
just demo         # msgvault-core's generated demo archive
just check        # load every mod and provider headlessly
```

Without an archive it opens the demo archive by itself, and the title bar says
"Sample". It only ever reads: SQLite is opened read-only.

`cargo run` (a debug build) adds Moonframe's developer tools: the Components
menu, Inspect Element and Reload Modules. A release build (`just run`) leaves
them out, as the app's users see it; `--dev` brings them back.

Moonframe comes from GitHub, pinned in `Cargo.toml`, with only the features
this app uses. To work on both at once,
patch it to a checkout:

```toml
[patch."https://github.com/penso/moonframe"]
moonframe = { path = "../moonframe/crates/moonframe" }
```

## Develop

`just format` (nightly rustfmt and taplo), then `just ci`: formatting,
lockfile, clippy with `-D warnings`, tests, and the headless check. `just deny`
checks licences and advisories.

MIT licensed. msgvault's schema in `crates/msgvault-core/schema` keeps its own
licence.
