//! Scramble mode, for sharing screenshots of a real archive: every name,
//! address, subject, body, label and filename the views get is swapped
//! for a made-up one of the same shape. The same input always gives the
//! same output, so a person keeps one name across the list, the reader and
//! the attachments, and a thread still reads as one conversation.
//!
//! Text is replaced word by word (a word of about the same length, its
//! capital kept), digits by other digits, and punctuation and spacing are
//! kept, so the layout looks as it did. Nothing real is kept: no word, no
//! number, no domain.

use {
    sha2::{Digest, Sha256},
    std::{
        collections::hash_map::RandomState,
        fs,
        hash::{BuildHasher, Hasher},
        io::Read,
        path::Path,
    },
};

const FIRST: [&str; 40] = [
    "Ada", "Bruno", "Clara", "Dmitri", "Elena", "Felix", "Greta", "Hugo", "Ines", "Jonas", "Keiko",
    "Liam", "Maya", "Nico", "Olga", "Pablo", "Quinn", "Rosa", "Sven", "Tara", "Uma", "Victor",
    "Wren", "Xavi", "Yara", "Zoe", "Amir", "Bea", "Cyril", "Dana", "Emil", "Fatima", "Gus", "Hana",
    "Ivo", "June", "Kai", "Lena", "Milo", "Nora",
];

const LAST: [&str; 40] = [
    "Abbott",
    "Barros",
    "Castell",
    "Dorsey",
    "Eklund",
    "Ferreira",
    "Gallo",
    "Hart",
    "Ivanova",
    "Jansen",
    "Kowalski",
    "Lindqvist",
    "Moreau",
    "Nakamura",
    "Okafor",
    "Petrov",
    "Quist",
    "Rossi",
    "Sato",
    "Thorne",
    "Ueda",
    "Vance",
    "Weber",
    "Xu",
    "Yilmaz",
    "Zeller",
    "Alvarez",
    "Brandt",
    "Costa",
    "Duval",
    "Engel",
    "Fischer",
    "Grant",
    "Holm",
    "Iqbal",
    "Jung",
    "Klein",
    "Lopez",
    "Mercer",
    "Novak",
];

const DOMAINS: [&str; 6] = [
    "example.com",
    "example.org",
    "example.net",
    "mail.example",
    "corp.example",
    "studio.example",
];

/// Made-up words by length, 1 to 9 letters; longer words take two.
const WORDS: [&[&str]; 9] = [
    &["a", "o", "i"],
    &["to", "of", "in", "on", "at", "we", "by", "so"],
    &[
        "the", "and", "for", "new", "our", "via", "now", "per", "set", "map",
    ],
    &[
        "plan", "note", "team", "week", "call", "deck", "list", "item", "next", "goal",
    ],
    &[
        "draft", "quick", "about", "board", "point", "chart", "notes", "share", "brief", "trial",
    ],
    &[
        "review", "update", "agenda", "sketch", "status", "metric", "budget", "launch", "design",
    ],
    &[
        "project", "meeting", "summary", "roadmap", "quarter", "release", "invoice", "minutes",
    ],
    &[
        "schedule", "proposal", "timeline", "workshop", "followup", "estimate", "planning",
    ],
    &[
        "milestone",
        "checklist",
        "itinerary",
        "follow-up",
        "reminders",
        "agreement",
    ],
];

/// A keyed hash: SHA-256 of this install's secret and `text`. Without
/// the secret, which never leaves this machine, nobody can work out what a
/// made-up name stands for, nor check a guess against a screenshot.
fn hash(key: &Key, text: &str) -> u64 {
    let digest = Sha256::new()
        .chain_update(key)
        .chain_update(text.as_bytes())
        .finalize();
    let mut first = [0_u8; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(first)
}

/// This install's secret for [`hash`].
pub(crate) type Key = [u8; 32];

/// The secret kept in `file`, made the first time: random bytes from the
/// system (`/dev/urandom`), else from the process's own random hasher keys.
pub(crate) fn key(file: &Path) -> Key {
    if let Ok(bytes) = fs::read(file)
        && let Ok(key) = Key::try_from(bytes.as_slice())
    {
        return key;
    }
    let mut key = [0_u8; 32];
    let random = fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut key));
    if random.is_err() {
        for (index, chunk) in key.chunks_mut(8).enumerate() {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_usize(index);
            chunk.copy_from_slice(&hasher.finish().to_le_bytes());
        }
    }
    let kept = file
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(file, key));
    if let Err(error) = kept {
        tracing::warn!(%error, "scramble: cannot keep the secret; names change next launch");
    }
    key
}

fn pick<'a>(list: &[&'a str], seed: u64) -> &'a str {
    list[(seed % list.len() as u64) as usize]
}

/// Whether to scramble, and with which secret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mask {
    pub(crate) on: bool,
    pub(crate) key: Key,
}

impl Mask {
    /// A person's name: `Ada Lovelace` becomes, say, `Nora Okafor`.
    pub(crate) fn name(self, name: &str) -> String {
        if !self.on || name.trim().is_empty() {
            return name.to_owned();
        }
        if name.contains('@') {
            return self.address(name);
        }
        if name.chars().any(|c| c.is_ascii_digit()) && !name.chars().any(char::is_alphabetic) {
            return self.text(name);
        }
        let seed = hash(&self.key, &name.trim().to_lowercase());
        format!("{} {}", pick(&FIRST, seed), pick(&LAST, seed >> 20))
    }

    /// An address: an email becomes one at an example domain, from the
    /// same made-up name its owner gets; a phone number other digits.
    pub(crate) fn address(self, address: &str) -> String {
        if !self.on || address.trim().is_empty() {
            return address.to_owned();
        }
        let Some((local, _)) = address.split_once('@') else {
            return self.text(address);
        };
        let seed = hash(&self.key, &address.trim().to_lowercase());
        let first = pick(&FIRST, seed).to_lowercase();
        let last = pick(&LAST, seed >> 20).to_lowercase();
        let local = if local.contains('.') {
            format!("{first}.{last}")
        } else {
            first
        };
        format!("{local}@{}", pick(&DOMAINS, seed >> 40))
    }

    /// A sender's address, from the made-up name `name` gets, so the two
    /// agree: `Nora Okafor` writes from `nora.okafor@example.org`.
    pub(crate) fn address_of(self, name: &str, address: &str) -> String {
        if !self.on || address.trim().is_empty() || !address.contains('@') {
            return self.address(address);
        }
        if name.trim().is_empty() || name.contains('@') {
            return self.address(address);
        }
        let made = self.name(name).to_lowercase().replace(' ', ".");
        let seed = hash(&self.key, &address.trim().to_lowercase());
        format!("{made}@{}", pick(&DOMAINS, seed >> 40))
    }

    /// Text: each word swapped for a made-up one of about its length (its
    /// capital kept), each digit for another; the rest kept.
    pub(crate) fn text(self, text: &str) -> String {
        if !self.on {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            if !word.is_empty() {
                out.push_str(&swap(&self.key, word));
                word.clear();
            }
        };
        for c in text.chars() {
            if c.is_alphabetic() {
                word.push(c);
                continue;
            }
            flush(&mut word, &mut out);
            if let Some(digit) = c.to_digit(10) {
                let seed = hash(&self.key, &format!("{}{}", out.len(), digit));
                out.push(char::from(b'0' + (seed % 10) as u8));
            } else {
                out.push(c);
            }
        }
        flush(&mut word, &mut out);
        out
    }

    /// A label of the user's own: one or two made-up words.
    pub(crate) fn label(self, label: &str) -> String {
        if !self.on {
            return label.to_owned();
        }
        let mut words = label.split('/').map(|part| {
            let word = swap(&self.key, part);
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        });
        let first: String = words.next().unwrap_or_default();
        words.fold(first, |path, part: String| format!("{path}/{part}"))
    }

    /// A filename: made-up words for its stem, its extension kept.
    pub(crate) fn file(self, name: &str) -> String {
        if !self.on {
            return name.to_owned();
        }
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => (stem, Some(ext)),
            _ => (name, None),
        };
        let stem = self.text(stem).to_lowercase();
        match ext {
            Some(ext) => format!("{stem}.{ext}"),
            None => stem,
        }
    }
}

/// A made-up word for `word`, about as long, with its first capital kept.
fn swap(key: &Key, word: &str) -> String {
    // A reply or forward still reads as one.
    if matches!(word.to_lowercase().as_str(), "re" | "fwd" | "fw") {
        return word.to_owned();
    }
    let length = word.chars().count();
    let seed = hash(key, &word.to_lowercase());
    let made = if length <= WORDS.len() {
        // Never the word itself, should the list happen to have it.
        let words = WORDS[length - 1];
        let lower = word.to_lowercase();
        let first = pick(words, seed);
        if first == lower {
            pick(words, seed + 1)
        } else {
            first
        }
        .to_owned()
    } else {
        let half = length / 2;
        format!(
            "{}{}",
            pick(WORDS[(half - 1).min(WORDS.len() - 1)], seed),
            pick(WORDS[(length - half - 1).min(WORDS.len() - 1)], seed >> 32)
        )
    };
    let capital = word.chars().next().is_some_and(char::is_uppercase);
    let shout = length > 1 && word.chars().all(|c| !c.is_lowercase());
    if shout {
        made.to_uppercase()
    } else if capital {
        let mut chars = made.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    } else {
        made
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const ON: Mask = Mask {
        on: true,
        key: [7; 32],
    };

    #[test]
    fn off_changes_nothing() {
        let off = Mask::default();
        assert_eq!(off.name("Ada Lovelace"), "Ada Lovelace");
        assert_eq!(off.text("Lunch at 12?"), "Lunch at 12?");
    }

    #[test]
    fn names_and_addresses_are_made_up_and_stable() {
        let name = ON.name("Priya Raman");
        assert_ne!(name, "Priya Raman");
        assert_eq!(name, ON.name("priya raman"));
        assert_eq!(name.split(' ').count(), 2);
        let address = ON.address("priya@lumen.dev");
        assert!(
            address.ends_with("example") || address.contains("@example."),
            "{address}"
        );
        assert!(!address.contains("lumen") && !address.contains("priya"));
        assert_eq!(address, ON.address("priya@lumen.dev"));
    }

    #[test]
    fn text_keeps_its_shape_and_nothing_real() {
        let text = "Invoice #2024-118 from Okafor Design: $1,250 due Friday.";
        let scrambled = ON.text(text);
        assert!(scrambled.chars().count() > 20);
        for real in [
            "Invoice", "2024", "118", "Okafor", "Design", "1,250", "Friday",
        ] {
            assert!(!scrambled.contains(real), "{real} in {scrambled}");
        }
        assert!(scrambled.contains('#') && scrambled.contains(": $") && scrambled.ends_with('.'));
        assert_eq!(scrambled, ON.text(text));
        assert!(ON.text("Hello").starts_with(char::is_uppercase));
        assert!(ON.text("Re: Lunch").starts_with("Re: "));
        let (name, address) = (
            ON.name("Priya Raman"),
            ON.address_of("Priya Raman", "priya@lumen.dev"),
        );
        assert!(
            address.starts_with(&name.to_lowercase().replace(' ', ".")),
            "{name} {address}"
        );
    }

    #[test]
    fn another_secret_gives_other_names() {
        let other = Mask {
            on: true,
            key: [8; 32],
        };
        let names = ["Ada Lovelace", "Priya Raman", "Alan Turing", "Grace Hopper"];
        assert!(names.iter().any(|name| ON.name(name) != other.name(name)));
        assert_ne!(ON.text("1234567890"), other.text("1234567890"));
    }

    #[test]
    fn the_secret_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("scramble.key");
        let made = key(&file);
        assert_ne!(made, [0; 32]);
        assert_eq!(key(&file), made);
    }

    #[test]
    fn files_and_labels_keep_their_kind() {
        let file = ON.file("atlas-roadmap-q3.pdf");
        assert!(file.ends_with(".pdf") && !file.contains("atlas"), "{file}");
        let label = ON.label("Projects/Atlas");
        assert!(label.contains('/') && !label.contains("Atlas"), "{label}");
    }
}
