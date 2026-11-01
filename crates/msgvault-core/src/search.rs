//! A subset of msgvault's Gmail-like search syntax, for the SQLite reader.
//! The daemon backend forwards the raw query and lets msgvault parse it.

use chrono::NaiveDate;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Free-text terms and quoted phrases, matched with full-text search.
    pub text: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub subject: Vec<String>,
    pub labels: Vec<String>,
    pub has_attachment: bool,
    pub unread: bool,
    pub starred: bool,
    pub after: Option<NaiveDate>,
    pub before: Option<NaiveDate>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let mut query = Self::default();
        for token in tokenize(input) {
            let Some((key, value)) = token.split_once(':').filter(|_| !token.quoted) else {
                query.text.push(token.text);
                continue;
            };
            let value = value.trim_matches('"').to_owned();
            if value.is_empty() {
                query.text.push(token.text.clone());
                continue;
            }
            match key.to_ascii_lowercase().as_str() {
                "from" => query.from.push(value),
                "to" | "cc" | "bcc" => query.to.push(value),
                "subject" => query.subject.push(value),
                "label" | "l" | "in" => query.labels.push(value),
                "has" if value.eq_ignore_ascii_case("attachment") => query.has_attachment = true,
                "is" if value.eq_ignore_ascii_case("unread") => query.unread = true,
                "is" if value.eq_ignore_ascii_case("starred") => query.starred = true,
                "after" | "since" => query.after = parse_date(&value),
                "before" | "until" => query.before = parse_date(&value),
                _ => query.text.push(token.text.clone()),
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// An FTS5 MATCH expression: each term quoted and prefix-matched, so user
    /// punctuation cannot inject FTS operators.
    pub fn fts_expression(&self) -> Option<String> {
        let terms: Vec<String> = self
            .text
            .iter()
            .filter(|term| term.chars().any(char::is_alphanumeric))
            .map(|term| format!("\"{}\"*", term.replace('*', "").replace('"', "\"\"")))
            .collect();
        (!terms.is_empty()).then(|| terms.join(" "))
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y/%m/%d"))
        .ok()
}

struct Token {
    text: String,
    /// The whole token was a quoted phrase, so `a:b` inside it is literal.
    quoted: bool,
}

impl std::ops::Deref for Token {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

/// Splits on whitespace, keeping `"quoted phrases"` and `key:"quoted values"`.
fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quoted = false;
    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                if current.is_empty() {
                    quoted = true;
                } else if !quoted {
                    current.push(ch);
                }
            },
            c if c.is_whitespace() && !in_quotes => {
                push(&mut tokens, &mut current, &mut quoted);
            },
            c => current.push(c),
        }
    }
    push(&mut tokens, &mut current, &mut quoted);
    tokens
}

fn push(tokens: &mut Vec<Token>, current: &mut String, quoted: &mut bool) {
    if !current.is_empty() {
        tokens.push(Token {
            text: std::mem::take(current),
            quoted: *quoted,
        });
    }
    *quoted = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operators_and_text() {
        let q = SearchQuery::parse(
            r#"invoice from:ada@example.com subject:"q3 report" label:Work has:attachment after:2024-01-01 "exact phrase" is:unread"#,
        );
        assert_eq!(q.text, vec!["invoice", "exact phrase"]);
        assert_eq!(q.from, vec!["ada@example.com"]);
        assert_eq!(q.subject, vec!["q3 report"]);
        assert_eq!(q.labels, vec!["Work"]);
        assert!(q.has_attachment && q.unread && !q.starred);
        assert_eq!(q.after, NaiveDate::from_ymd_opt(2024, 1, 1));
        assert_eq!(
            q.fts_expression().as_deref(),
            Some(r#""invoice"* "exact phrase"*"#)
        );
    }

    #[test]
    fn fts_expression_neutralises_operators() {
        let q = SearchQuery::parse(r#"NEAR(x) -y* ab"c --"#);
        assert_eq!(
            q.fts_expression().as_deref(),
            Some(r#""NEAR(x)"* "-y"* "ab""c --"*"#)
        );
        assert!(SearchQuery::parse("   ").is_empty());
    }

    #[test]
    fn unknown_operators_stay_text() {
        let q = SearchQuery::parse("http://x.test from:");
        assert_eq!(q.text, vec!["http://x.test", "from:"]);
    }
}
