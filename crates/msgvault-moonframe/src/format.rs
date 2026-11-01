//! Display formatting, as msgvault-gpui's `format.rs`: dates, sizes,
//! counts, label colours, avatars and file kinds, ready for the Luau views.

use chrono::{DateTime, Datelike, Local, Utc};

/// Compact list timestamp: time today, weekday this week, date otherwise.
pub(crate) fn list_date(time: Option<DateTime<Utc>>, now: DateTime<Local>) -> String {
    let Some(time) = time else {
        return String::new();
    };
    let local = time.with_timezone(&Local);
    let days = now
        .date_naive()
        .signed_duration_since(local.date_naive())
        .num_days();
    if days == 0 {
        clock(time)
    } else if days == 1 {
        "Yesterday".into()
    } else if (2..7).contains(&days) {
        local.format("%A").to_string()
    } else if local.year() == now.year() {
        local.format("%b %-d").to_string()
    } else {
        local.format("%b %-d, %Y").to_string()
    }
}

/// Reader timestamp, e.g. `Tue, Mar 5, 2024 at 9:41 AM`.
pub(crate) fn full_date(time: Option<DateTime<Utc>>) -> String {
    time.map(|time| {
        time.with_timezone(&Local)
            .format("%a, %b %-d, %Y at %-I:%M %p")
            .to_string()
    })
    .unwrap_or_default()
}

/// `9:41 AM`.
pub(crate) fn clock(time: DateTime<Utc>) -> String {
    time.with_timezone(&Local).format("%-I:%M %p").to_string()
}

pub(crate) fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// `12345` as `12,345`.
pub(crate) fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn plural(n: u64, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        count(n),
        if n == 1 {
            one
        } else {
            many
        }
    )
}

fn fnv(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}

/// A label's colour as `#rrggbb`: the stored one, else a stable pick.
pub(crate) fn label_color(name: &str, stored: Option<&str>) -> String {
    if let Some(color) = stored.map(str::trim).filter(|color| {
        color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return color.to_ascii_lowercase();
    }
    const PALETTE: [&str; 10] = [
        "#4a86e8", "#8e63ce", "#16a766", "#f2a60c", "#e66550", "#2da2bb", "#d5608c", "#3dc789",
        "#7a8cff", "#c28b3c",
    ];
    PALETTE[fnv(name) as usize % PALETTE.len()].to_owned()
}

/// Up to two initials for an avatar, as gpui-kit's Avatar draws them.
pub(crate) fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(' ').filter(|word| !word.is_empty()).collect();
    let mut text: String = words
        .iter()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect();
    if text.chars().count() == 1 {
        text = name.chars().take(2).collect();
    }
    if text.is_empty() {
        text = "?".into();
    }
    text.to_uppercase()
}

/// The avatar's hue, 0 to 11.
pub(crate) fn hue(initials: &str) -> i64 {
    i64::from(fnv(initials) % 12)
}

/// Filenames from mail headers are untrusted: keep only a safe final component.
pub(crate) fn safe_filename(raw: &str, fallback: &str) -> String {
    let name: String = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_start_matches('.').to_owned();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name
    }
}

/// An icon for a source of messages that isn't email.
pub(crate) fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "calendar_event" => "calendar-days",
        "meeting_transcript" => "message-square",
        "slack" | "discord" | "teams" | "google_chat" | "matrix" => "hash",
        _ => "messages-square",
    }
}

/// An icon for an account, by its kind.
pub(crate) fn account_icon(kind: &str) -> &'static str {
    match kind {
        "gmail" | "imap" | "fastmail" | "microsoft" | "outlook" | "mbox" | "emlx" | "pst" => "mail",
        "whatsapp" | "imessage" | "apple_messages" | "google_messages" | "sms" | "beeper"
        | "google_voice" => "message-square",
        "slack" | "discord" | "teams" | "google_chat" | "matrix" => "hash",
        "gcal" | "calendar" => "calendar-days",
        _ => "database",
    }
}

/// The attachment browser's kinds: id, title, icon.
pub(crate) const FILE_KINDS: [(&str, &str, &str); 6] = [
    ("image", "Images", "image"),
    ("pdf", "PDFs", "file-text"),
    ("document", "Documents", "file-text"),
    ("calendar", "Calendar", "calendar-days"),
    ("archive", "Archives", "file-archive"),
    ("other", "Other", "file"),
];

/// What kind of file it is, for the browser's filters and the icon.
pub(crate) fn file_kind(mime: &str, name: &str) -> &'static str {
    let mime = mime.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    if mime.starts_with("image/")
        || matches!(
            ext,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" | "tif" | "tiff"
        )
    {
        "image"
    } else if mime.contains("pdf") || ext == "pdf" {
        "pdf"
    } else if mime == "text/calendar" || ext == "ics" {
        "calendar"
    } else if ["zip", "compressed", "tar", "gzip", "7z", "rar"]
        .iter()
        .any(|word| mime.contains(word))
        || matches!(ext, "zip" | "tar" | "gz" | "tgz" | "7z" | "rar")
    {
        "archive"
    } else if mime.starts_with("text/")
        || [
            "document",
            "sheet",
            "presentation",
            "msword",
            "excel",
            "powerpoint",
            "rtf",
            "json",
            "xml",
            "csv",
        ]
        .iter()
        .any(|word| mime.contains(word))
        || matches!(
            ext,
            "txt"
                | "md"
                | "csv"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "pages"
                | "numbers"
                | "key"
                | "rtf"
                | "odt"
                | "ods"
                | "json"
        )
    {
        "document"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use {super::*, chrono::TimeZone};

    #[test]
    fn counts_and_sizes() {
        assert_eq!(count(0), "0");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(300 * 1024 * 1024), "300 MB");
        assert_eq!(plural(1, "message", "messages"), "1 message");
    }

    #[test]
    fn list_dates_are_relative() {
        let Some(now) = Local.with_ymd_and_hms(2024, 6, 14, 18, 0, 0).single() else {
            return;
        };
        let at = |days: i64| Some((now - chrono::Duration::days(days)).with_timezone(&Utc));
        assert!(list_date(at(0), now).ends_with("PM"));
        assert_eq!(list_date(at(1), now), "Yesterday");
        assert_eq!(list_date(at(3), now), "Tuesday");
        assert_eq!(list_date(at(40), now), "May 5");
        assert_eq!(list_date(at(400), now), "May 11, 2023");
        assert_eq!(list_date(None, now), "");
    }

    #[test]
    fn filenames_cannot_escape() {
        assert_eq!(safe_filename("../../etc/passwd", "f"), "passwd");
        assert_eq!(safe_filename("..", "f"), "f");
        assert_eq!(safe_filename("a:b?.pdf", "f"), "a_b_.pdf");
    }

    #[test]
    fn colors_avatars_and_kinds() {
        assert_eq!(label_color("Work", Some("#4A86E8")), "#4a86e8");
        assert_eq!(label_color("Work", None), label_color("Work", Some("blue")));
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("ada"), "AD");
        assert_eq!(initials(""), "?");
        assert_eq!(file_kind("application/pdf", "x"), "pdf");
        assert_eq!(file_kind("", "photo.JPG"), "image");
        assert_eq!(file_kind("application/octet-stream", "blob"), "other");
    }
}
