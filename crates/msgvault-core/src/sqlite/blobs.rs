//! Attachment content lives in a content-addressed store under
//! `<data>/attachments`: loose files at `<sha[:2]>/<sha>`, or sealed packs at
//! `packs/<id[:2]>/<id>.mvpack` located through `attachment_pack_index`
//! (`internal/store/packs.go`). Pack entries with flag bit 0 are zstd frames.

use {
    super::Schema,
    crate::{Attachment, Error, Result},
    rusqlite::{Connection, OptionalExtension},
    sha2::{Digest, Sha256},
    std::{
        fs::File,
        io::{Read, Seek, SeekFrom},
        path::{Path, PathBuf},
    },
};

/// Largest attachment loaded into memory for viewing or saving.
pub(crate) const ATTACHMENT_LIMIT: u64 = 512 * 1024 * 1024;
const FLAG_ZSTD: i64 = 1;

struct PackEntry {
    pack_id: String,
    offset: u64,
    stored_len: u64,
    raw_len: u64,
    flags: i64,
}

pub(crate) fn read(
    conn: &Connection,
    schema: Schema,
    root: &Path,
    attachment: &Attachment,
) -> Result<Vec<u8>> {
    if let Some(url) = attachment.remote_url() {
        return Err(Error::RemoteAttachment(url.to_owned()));
    }
    let hash = content_hash(attachment).ok_or(Error::AttachmentMissing)?;
    let entry = if schema.pack_index {
        pack_entry(conn, &hash)?
    } else {
        None
    };
    let bytes = match entry {
        Some(entry) => read_packed(root, &entry)?,
        None => read_loose(root, &hash, attachment.storage_path.as_deref())?,
    };
    verify(&hash, &bytes)?;
    Ok(bytes)
}

/// The SHA-256 key: `content_hash`, else the file name of `storage_path`.
pub(crate) fn content_hash(attachment: &Attachment) -> Option<String> {
    let is_hash = |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit());
    attachment
        .content_hash
        .as_deref()
        .filter(|hash| is_hash(hash))
        .or_else(|| {
            attachment
                .storage_path
                .as_deref()
                .and_then(|path| path.rsplit('/').next())
                .filter(|name| is_hash(name))
        })
        .map(str::to_ascii_lowercase)
}

fn pack_entry(conn: &Connection, hash: &str) -> Result<Option<PackEntry>> {
    Ok(conn
        .query_row(
            "SELECT pack_id, pack_offset, stored_len, raw_len, flags
             FROM attachment_pack_index WHERE blob_hash = ?1",
            [hash],
            |row| {
                Ok(PackEntry {
                    pack_id: row.get(0)?,
                    offset: row.get::<_, i64>(1)?.max(0) as u64,
                    stored_len: row.get::<_, i64>(2)?.max(0) as u64,
                    raw_len: row.get::<_, i64>(3)?.max(0) as u64,
                    flags: row.get(4)?,
                })
            },
        )
        .optional()?)
}

fn shard(root: &Path, key: &str) -> Option<PathBuf> {
    let prefix = key.get(..2)?;
    Some(root.join(prefix).join(key))
}

fn read_loose(root: &Path, hash: &str, storage_path: Option<&str>) -> Result<Vec<u8>> {
    let mut candidates: Vec<PathBuf> = shard(root, hash).into_iter().collect();
    // Legacy rows may store a different relative layout; never escape the root.
    if let Some(relative) = storage_path.filter(|p| !p.contains("..") && !p.starts_with('/')) {
        candidates.push(root.join(relative));
    }
    for path in candidates {
        match File::open(&path) {
            Ok(file) => return read_limited(file, &path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(error) => return Err(Error::io(path, error)),
        }
    }
    Err(Error::AttachmentMissing)
}

fn read_packed(root: &Path, entry: &PackEntry) -> Result<Vec<u8>> {
    if entry.stored_len > ATTACHMENT_LIMIT || entry.raw_len > ATTACHMENT_LIMIT {
        return Err(Error::TooLarge {
            limit: ATTACHMENT_LIMIT,
        });
    }
    let file_name = format!("{}.mvpack", entry.pack_id);
    let path = shard(&root.join("packs"), &entry.pack_id)
        .map(|dir| dir.with_file_name(&file_name))
        .ok_or(Error::AttachmentMissing)?;
    let mut file = File::open(&path).map_err(|error| Error::io(&path, error))?;
    file.seek(SeekFrom::Start(entry.offset))
        .map_err(|error| Error::io(&path, error))?;
    let mut stored = vec![0; entry.stored_len as usize];
    file.read_exact(&mut stored)
        .map_err(|error| Error::io(&path, error))?;
    if entry.flags & FLAG_ZSTD == 0 {
        return Ok(stored);
    }
    let mut raw = Vec::with_capacity(entry.raw_len as usize);
    zstd::stream::read::Decoder::new(stored.as_slice())
        .map_err(Error::Decompress)?
        .take(ATTACHMENT_LIMIT + 1)
        .read_to_end(&mut raw)
        .map_err(Error::Decompress)?;
    Ok(raw)
}

fn read_limited(file: File, path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(ATTACHMENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::io(path, error))?;
    if bytes.len() as u64 > ATTACHMENT_LIMIT {
        return Err(Error::TooLarge {
            limit: ATTACHMENT_LIMIT,
        });
    }
    Ok(bytes)
}

fn verify(expected: &str, bytes: &[u8]) -> Result<()> {
    let actual = hex(&Sha256::digest(bytes));
    if actual != expected {
        return Err(Error::AttachmentCorrupt {
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use {super::*, crate::AttachmentId};

    fn attachment(hash: Option<&str>, path: Option<&str>) -> Attachment {
        Attachment {
            id: AttachmentId(1),
            filename: "a.txt".into(),
            mime_type: "text/plain".into(),
            size_bytes: 1,
            content_hash: hash.map(Into::into),
            storage_path: path.map(Into::into),
            inline: false,
        }
    }

    #[test]
    fn hash_falls_back_to_storage_path() {
        let hash = "ab".repeat(32);
        let from_path = attachment(None, Some(&format!("ab/{hash}")));
        assert_eq!(content_hash(&from_path), Some(hash.clone()));
        let upper = attachment(Some(&hash.to_uppercase()), None);
        assert_eq!(content_hash(&upper), Some(hash));
        assert_eq!(content_hash(&attachment(Some("short"), None)), None);
    }

    #[test]
    fn remote_attachments_are_reported() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        let remote = attachment(None, Some("https://files.example/a"));
        assert!(matches!(
            read(&conn, Schema::default(), Path::new("/nonexistent"), &remote),
            Err(Error::RemoteAttachment(url)) if url == "https://files.example/a"
        ));
        Ok(())
    }
}
