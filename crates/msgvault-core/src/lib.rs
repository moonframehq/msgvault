//! Read access to [msgvault](https://github.com/wesm/msgvault) archives.
//!
//! msgvault is a Go program with no embeddable library, so this crate offers
//! two interchangeable [`Backend`]s: a direct read-only reader of
//! `msgvault.db`, and a client of the `msgvault serve` REST API. Nothing here
//! depends on a UI toolkit.

mod backend;
mod error;
pub mod fixture;
mod http;
mod location;
mod model;
mod search;
mod sqlite;
mod time;

pub use {
    backend::{Backend, BackendChoice, BackendInfo, BackendKind, open},
    error::{Error, Result},
    http::HttpBackend,
    location::{ArchiveLocation, display_path, expand_tilde},
    model::*,
    search::SearchQuery,
    sqlite::SqliteBackend,
    time::parse_timestamp,
};
