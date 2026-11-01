//! `message_raw` holds the original payload, zlib-compressed by default
//! (`internal/query/shared.go`). Only MIME payloads carry a mail body.

use {
    crate::{Error, MessageId, Result},
    flate2::read::ZlibDecoder,
    rusqlite::{Connection, OptionalExtension},
    std::io::Read,
};

/// Raw messages larger than this are not decoded for display.
const RAW_LIMIT: u64 = 64 * 1024 * 1024;

pub(crate) type Bodies = (Option<String>, Option<String>);

pub(crate) fn bodies(conn: &Connection, id: MessageId) -> Result<Option<Bodies>> {
    let Some(bytes) = mime(conn, id)? else {
        return Ok(None);
    };
    let Some(message) = mail_parser::MessageParser::default().parse(&bytes) else {
        return Ok(None);
    };
    // mail-parser synthesises a missing text or HTML alternative from the
    // other one; keep only parts that really exist in the message.
    let is_html = |id: &mail_parser::MessagePartId| {
        message
            .parts
            .get(*id as usize)
            .is_some_and(|part| matches!(part.body, mail_parser::PartType::Html(_)))
    };
    let text = message
        .text_body
        .first()
        .filter(|id| !is_html(id))
        .and_then(|_| message.body_text(0))
        .map(|text| text.into_owned());
    let html = message
        .html_body
        .first()
        .filter(|id| is_html(id))
        .and_then(|_| message.body_html(0))
        .map(|html| html.into_owned());
    Ok(Some((text, html)))
}

/// The raw RFC 822 bytes of an email, when archived.
pub(crate) fn mime(conn: &Connection, id: MessageId) -> Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>, Option<String>, String)> = conn
        .query_row(
            "SELECT raw_data, compression, raw_format FROM message_raw WHERE message_id = ?1",
            [id.0],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((data, compression, format)) = row else {
        return Ok(None);
    };
    if format != "mime" {
        return Ok(None);
    }
    decompress(data, compression.as_deref()).map(Some)
}

pub(crate) fn decompress(data: Vec<u8>, compression: Option<&str>) -> Result<Vec<u8>> {
    match compression {
        Some("zlib") => {
            let mut out = Vec::new();
            ZlibDecoder::new(data.as_slice())
                .take(RAW_LIMIT + 1)
                .read_to_end(&mut out)
                .map_err(Error::Decompress)?;
            if out.len() as u64 > RAW_LIMIT {
                return Err(Error::TooLarge { limit: RAW_LIMIT });
            }
            Ok(out)
        },
        _ => Ok(data),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        flate2::{Compression, write::ZlibEncoder},
        std::io::Write,
    };

    #[test]
    fn zlib_round_trips_and_plain_passes_through() -> Result<()> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hello").map_err(Error::Decompress)?;
        let compressed = encoder.finish().map_err(Error::Decompress)?;
        assert_eq!(decompress(compressed, Some("zlib"))?, b"hello");
        assert_eq!(decompress(b"raw".to_vec(), None)?, b"raw");
        assert!(matches!(
            decompress(b"not zlib".to_vec(), Some("zlib")),
            Err(Error::Decompress(_))
        ));
        Ok(())
    }
}
