//! HTML mail for the reader: a page for its web view, kept safe, and a
//! plain rendering for the text view.

use {
    regex::Regex,
    std::{
        fs, io,
        path::{Path, PathBuf},
        sync::LazyLock,
    },
};

macro_rules! pattern {
    ($name:ident, $source:literal) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new($source).unwrap_or_else(|error| panic!("bad pattern {}: {error}", $source))
        });
    };
}

pattern!(
    DROPPED,
    r"(?is)<(style|script|head)\b.*?</(?:style|script|head)\s*>"
);
pattern!(ITEM, r"(?i)<li\b[^>]*>");
pattern!(
    BREAK,
    r"(?i)</?(?:br|p|div|tr|li|h[1-4]|blockquote|table)\b[^>]*>"
);
pattern!(CELL, r"(?i)</?(?:td|th)\b[^>]*>");
pattern!(TAG, r"<[^>]*>");
pattern!(SPACE, r"\s+");
pattern!(
    REMOTE,
    r#"(?i)(?:src|background|srcset)\s*=\s*["']?\s*(?:https?:)?//|url\(\s*['"]?\s*(?:https?:)?//"#
);
pattern!(STYLE, r"(?is)<style\b[^>]*>(.*?)</style\s*>");
pattern!(IMPORT, r"(?i)@import[^;]*;");
pattern!(BODY, r"(?is)<body\b[^>]*>(.*)</body\s*>");
pattern!(H1, r"(?i)<h1\b");
pattern!(H23, r"(?i)<h[23]\b");
pattern!(IMG, r"(?i)<img\b[^>]*>");
pattern!(IMG_HEIGHT, r#"(?i)height\s*=\s*["']?(\d+)"#);

fn entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// A snippet on one line, its entities decoded.
pub(crate) fn clean(text: &str) -> String {
    SPACE.replace_all(&entities(text), " ").trim().to_owned()
}

/// A readable plain rendering of an HTML body.
pub(crate) fn html_to_text(html: &str) -> String {
    let html = DROPPED.replace_all(html, "");
    let html = ITEM.replace_all(&html, "\n• ");
    let html = BREAK.replace_all(&html, "\n");
    let html = CELL.replace_all(&html, "\t");
    let html = entities(&TAG.replace_all(&html, ""));
    let mut lines: Vec<String> = Vec::new();
    for line in html.split('\n') {
        let line = line.trim_end().trim_start_matches('\t');
        let line = line
            .split('\t')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("  ");
        if line.is_empty() && lines.last().is_some_and(String::is_empty) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_owned()
}

/// Whether the HTML loads anything from the network (pictures, backgrounds).
pub(crate) fn remote_content(html: &str) -> bool {
    REMOTE.is_match(html)
}

/// How tall the page draws, roughly: the web view can't measure it.
pub(crate) fn height(html: &str) -> i64 {
    let mut height: i64 = 6;
    for line in html_to_text(html).lines() {
        height += if line.trim().is_empty() {
            6
        } else {
            23 * (1 + line.chars().count() as i64 / 95)
        };
    }
    height += 38 * H1.find_iter(html).count() as i64;
    height += 12 * H23.find_iter(html).count() as i64;
    for image in IMG.find_iter(html) {
        height += IMG_HEIGHT
            .captures(image.as_str())
            .and_then(|stated| stated[1].parse::<i64>().ok())
            .map_or(160, |stated| stated + 8);
    }
    height.clamp(60, 1400)
}

/// A message's HTML as a page for the reader's web view, written into
/// `dir`: its body and styles kept; scripts, frames, forms and refreshes
/// dropped; under a Content-Security-Policy that runs no script and loads
/// nothing from the network (nor remote pictures, unless `remote`); in the
/// window's light or dark colours. Returns the file.
pub(crate) fn write(
    dir: &Path,
    id: i64,
    html: &str,
    dark: bool,
    remote: bool,
) -> io::Result<PathBuf> {
    let file = dir.join(format!(
        "{id}-{}{}.html",
        if dark {
            "dark"
        } else {
            "light"
        },
        if remote {
            "-remote"
        } else {
            ""
        }
    ));
    if file.is_file() {
        return Ok(file);
    }
    // Only what is inside each style element, without a single `<` (CSS
    // never needs one), so nothing can close the element and follow it.
    let styles: Vec<String> = STYLE
        .captures_iter(html)
        .filter_map(|captures| captures.get(1))
        .map(|inner| {
            let css = IMPORT.replace_all(inner.as_str(), "").replace('<', "");
            format!("<style>{css}</style>")
        })
        .collect();
    let body = BODY
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map_or(html, |body| body.as_str());
    let body = sanitize(body);
    let images = if remote {
        "data: cid: https: http:"
    } else {
        "data: cid:"
    };
    let (bg, fg, muted, link, rule, scheme) = if dark {
        (
            "#0a0a0a", "#fafafa", "#a3a3a3", "#60a5fa", "#262626", "dark",
        )
    } else {
        (
            "#ffffff", "#0a0a0a", "#737373", "#2563eb", "#e5e5e5", "light",
        )
    };
    let kept = styles.join("\n");
    let page = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {images}; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'">
<meta name="color-scheme" content="{scheme}">
<style>
html, body {{ margin: 0; padding: 0; background: {bg}; color: {fg}; }}
body {{ font: 14px/1.6 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; overflow-wrap: anywhere; }}
a {{ color: {link}; }}
h1 {{ font-size: 2.15em; line-height: 1.2; margin: 0.15em 0 0.35em; }}
h2, h3 {{ line-height: 1.3; margin: 0.5em 0 0.3em; }}
p {{ margin: 0 0 0.5em; }}
ul, ol {{ margin: 0 0 0.5em; padding-left: 1.1em; }}
blockquote {{ margin: 0.3em 0 0.5em; padding-left: 1.4em; border-left: 3px solid {rule}; color: {muted}; }}
img {{ max-width: 100%; height: auto; }}
table {{ border-collapse: collapse; max-width: 100%; }}
td, th {{ padding: 2px 12px 2px 0; vertical-align: top; text-align: left; }}
small {{ color: {muted}; }}
</style>
{kept}
</head><body>{body}</body></html>
"#
    );
    fs::create_dir_all(dir)?;
    let partial = file.with_extension("html.tmp");
    fs::write(&partial, page)?;
    fs::rename(&partial, &file)?;
    Ok(file)
}

/// The body parsed once, as a browser would (html5ever, through ammonia),
/// keeping only an allowlist of tags and attributes: no script, handler,
/// frame, form, meta or base survives. Inline styles and the presentational
/// attributes mail is laid out with stay. The page's
/// Content-Security-Policy still runs no script whatever is left.
fn sanitize(body: &str) -> String {
    ammonia::Builder::default()
        .add_tags(["center", "font"])
        .add_generic_attributes([
            "style", "class", "id", "align", "valign", "width", "height", "bgcolor", "color",
            "border", "cellpadding", "cellspacing", "dir", "face", "size",
        ])
        .url_schemes(["http", "https", "mailto", "cid", "data"].into())
        .link_rel(Some("noopener noreferrer"))
        .clean(body)
        .to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_lines_and_drops_markup() {
        let html = "<html><head><style>p{}</style></head><body><p>Hi&nbsp;there</p>\
            <ul><li>one</li><li>two</li></ul><script>x()</script></body></html>";
        assert_eq!(html_to_text(html), "Hi there\n\n• one\n\n• two");
        assert_eq!(clean("  a &amp;\n b "), "a & b");
    }

    #[test]
    fn pages_run_no_script_and_load_nothing_remote_unless_asked() {
        let dir = tempfile::tempdir().unwrap();
        let html = r#"<body onload="x()"><script>alert(1)</script><img src="https://t.example/p.gif"><p>Hello</p></body>"#;
        assert!(remote_content(html));
        let page = fs::read_to_string(write(dir.path(), 7, html, false, false).unwrap()).unwrap();
        assert!(!page.contains("alert") && !page.contains("onload"));
        assert!(page.contains("img-src data: cid:;"));
        let nested = "<body><scr<script></script>ipt>alert(2)</script><p>Hi</p></body>";
        let page = fs::read_to_string(write(dir.path(), 8, nested, false, false).unwrap()).unwrap();
        assert!(!page.contains("<script"), "{page}");
        let breakout = "<style>p{}</style ><script>alert(3)</script><style></style><body>x</body>";
        let page = fs::read_to_string(write(dir.path(), 9, breakout, false, false).unwrap()).unwrap();
        assert!(!page.contains("alert(3)"), "{page}");
        let page = fs::read_to_string(write(dir.path(), 7, html, true, true).unwrap()).unwrap();
        assert!(page.contains("img-src data: cid: https: http:") && page.contains("#0a0a0a"));
    }
}
