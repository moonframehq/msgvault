//! Builds a synthetic msgvault archive for demos and tests.
//!
//! The database is created from msgvault's real schema (vendored in
//! `schema/`), then filled the way msgvault's importers fill it: Gmail system
//! and user labels, IMAP folder memberships with flags, chats, a calendar,
//! loose and packed attachments, an email known only by its raw MIME, and the
//! `messages_fts` index.

use {
    crate::{Error, Result, sqlite::hex},
    chrono::{DateTime, Duration, Utc},
    flate2::{Compression, write::ZlibEncoder},
    rusqlite::{Connection, OptionalExtension, params},
    sha2::{Digest, Sha256},
    std::{collections::HashMap, io::Write, path::Path},
};

const SCHEMA: &str = include_str!("../schema/schema.sql");
const SCHEMA_SQLITE: &str = include_str!("../schema/schema_sqlite.sql");
const PACK_ID: &str = "01kx758hcw5gnkdz233217fd9a";

/// What was generated, for tests and the CLI summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixtureSummary {
    pub accounts: usize,
    pub messages: usize,
    pub attachments: usize,
}

/// Writes `msgvault.db` and `attachments/` into `data_dir`, replacing any
/// previous fixture there. Dates are laid out backwards from `now`.
pub fn generate(data_dir: &Path, now: DateTime<Utc>) -> Result<FixtureSummary> {
    std::fs::create_dir_all(data_dir).map_err(|e| Error::io(data_dir, e))?;
    let db = data_dir.join("msgvault.db");
    for suffix in ["", "-wal", "-shm"] {
        let mut path = db.clone().into_os_string();
        path.push(suffix);
        match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(Error::io(path, error));
            },
            _ => {},
        }
    }
    let conn = Connection::open(&db).map_err(|source| Error::Open {
        path: db.clone(),
        source,
    })?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(SCHEMA_SQLITE)?;
    let mut builder = Builder {
        conn: &conn,
        root: data_dir.join("attachments"),
        now,
        rng: Rng(0x5eed_cafe_f00d_d00d),
        participants: HashMap::new(),
        labels: HashMap::new(),
        pack: Vec::new(),
        summary: FixtureSummary::default(),
        uid: 1,
    };
    conn.execute_batch("BEGIN")?;
    builder.populate()?;
    builder.finish_pack()?;
    conn.execute_batch("COMMIT")?;
    Ok(builder.summary)
}

/// Deterministic xorshift, so fixtures are identical across runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[derive(Clone, Copy)]
struct Person {
    name: &'static str,
    email: &'static str,
}

const ME_GMAIL: Person = Person {
    name: "Alex Morgan",
    email: "alex.morgan@gmail.com",
};
const ME_WORK: Person = Person {
    name: "Alex Morgan",
    email: "alex@morgan.studio",
};

const PEOPLE: &[Person] = &[
    Person {
        name: "Priya Raman",
        email: "priya@lumen.dev",
    },
    Person {
        name: "Jonas Becker",
        email: "jonas.becker@northwind.io",
    },
    Person {
        name: "Mara Okafor",
        email: "mara@okafor.design",
    },
    Person {
        name: "Sam Whitfield",
        email: "sam.whitfield@gmail.com",
    },
    Person {
        name: "Elena Petrova",
        email: "elena@petrova.law",
    },
    Person {
        name: "Diego Alvarez",
        email: "diego@fieldnotes.co",
    },
    Person {
        name: "Hannah Lee",
        email: "hannah.lee@icloud.com",
    },
    Person {
        name: "Tomás Rivera",
        email: "tomas@rivera.photo",
    },
    Person {
        name: "Ingrid Solberg",
        email: "ingrid@fjord.no",
    },
    Person {
        name: "Kenji Watanabe",
        email: "kenji@watanabe.jp",
    },
    Person {
        name: "Mom",
        email: "linda.morgan@outlook.com",
    },
    Person {
        name: "Chris Morgan",
        email: "chris.morgan@gmail.com",
    },
];

struct Sender {
    name: &'static str,
    email: &'static str,
    category: &'static str,
}

const NEWSLETTERS: &[Sender] = &[
    Sender {
        name: "The Rust Weekly",
        email: "hello@rustweekly.dev",
        category: "CATEGORY_UPDATES",
    },
    Sender {
        name: "Figma",
        email: "no-reply@figma.com",
        category: "CATEGORY_UPDATES",
    },
    Sender {
        name: "Patagonia",
        email: "news@patagonia.com",
        category: "CATEGORY_PROMOTIONS",
    },
    Sender {
        name: "Strava",
        email: "no-reply@strava.com",
        category: "CATEGORY_SOCIAL",
    },
    Sender {
        name: "Linear",
        email: "changelog@linear.app",
        category: "CATEGORY_UPDATES",
    },
    Sender {
        name: "Airbnb",
        email: "automated@airbnb.com",
        category: "CATEGORY_PROMOTIONS",
    },
];

/// One email conversation: subject, user labels, then (from_me, body) turns.
struct Script {
    subject: &'static str,
    labels: &'static [&'static str],
    turns: &'static [(bool, &'static str)],
    attachment: Option<(&'static str, &'static str)>,
}

const SCRIPTS: &[Script] = &[
    Script {
        subject: "Q3 roadmap review — notes and next steps",
        labels: &["Work", "Projects/Atlas"],
        turns: &[
            (
                false,
                "Hi Alex,\n\nThanks for running the roadmap review today. Summary of what we agreed:\n\n1. Atlas sync engine ships behind a flag on the 14th.\n2. Search v2 moves to Q4 so we can fix indexing latency first.\n3. We hire one more infra engineer before October.\n\nCould you own the write-up for the leadership sync on Thursday?\n\nPriya",
            ),
            (
                true,
                "Thanks Priya, great summary. I'll draft the write-up tomorrow morning and share it before lunch.\n\nOne thing to add: we should budget a week for migration tooling before the flag flips.\n\nAlex",
            ),
            (
                false,
                "Agreed on the migration week. Added it to the plan. Talk Thursday!",
            ),
        ],
        attachment: Some(("atlas-roadmap-q3.pdf", "application/pdf")),
    },
    Script {
        subject: "Your flight to Lisbon is confirmed ✈️",
        labels: &["Travel"],
        turns: &[(
            false,
            "<h2>Booking confirmed</h2><p>Hi Alex, your trip is all set.</p><table><tr><td><b>Flight</b></td><td>TP 202 · SFO → LIS</td></tr><tr><td><b>Departs</b></td><td>Fri 18:45</td></tr><tr><td><b>Seat</b></td><td>24A (window)</td></tr><tr><td><b>Confirmation</b></td><td>QX7LMP</td></tr></table><p>Check in opens 24 hours before departure. Have a great trip!</p><p><a href=\"https://example.com/manage\">Manage booking</a></p>",
        )],
        attachment: Some(("itinerary-QX7LMP.ics", "text/calendar")),
    },
    Script {
        subject: "Dinner on Saturday?",
        labels: &["Family"],
        turns: &[
            (
                false,
                "Hi sweetheart! Dad and I are in town this weekend. Are you free for dinner Saturday? We were thinking that little Italian place on Valencia.\n\nLove, Mom",
            ),
            (true, "Saturday works! 7pm? I'll book a table for three."),
            (false, "Perfect. Can't wait to see you ❤️"),
        ],
        attachment: None,
    },
    Script {
        subject: "Invoice #2024-118 from Okafor Design",
        labels: &["Receipts", "Work"],
        turns: &[(
            false,
            "Hi Alex,\n\nPlease find attached the invoice for the brand refresh work in August (logo system, typography and the icon set).\n\nTotal: $4,800.00 — due within 30 days.\n\nThank you, it was a joy working on this.\n\nMara",
        )],
        attachment: Some(("invoice-2024-118.pdf", "application/pdf")),
    },
    Script {
        subject: "Re: API rate limits for the partner integration",
        labels: &["Work"],
        turns: &[
            (
                false,
                "Alex — our partner is hitting 429s during their nightly batch. Can we raise their limit from 50 to 200 req/s for the 02:00–04:00 UTC window?",
            ),
            (
                true,
                "We can, but I'd rather give them a bulk endpoint. Let me check with the platform team on capacity first.",
            ),
            (
                false,
                "Sounds good. They're flexible as long as the batch finishes before 06:00.",
            ),
            (
                true,
                "Platform says 150 req/s is safe today, bulk endpoint in two sprints. I'll send them the details.",
            ),
        ],
        attachment: None,
    },
    Script {
        subject: "Photos from the coast trip 📸",
        labels: &["Family"],
        turns: &[
            (
                false,
                "Finally edited the photos from Big Sur! Here's a contact sheet — let me know which ones you want printed.",
            ),
            (
                true,
                "These are gorgeous. Number 7 and 12 please, maybe 8x10?",
            ),
        ],
        attachment: Some(("big-sur-contact-sheet.png", "image/png")),
    },
    Script {
        subject: "Contract redlines — Morgan Studio / Northwind",
        labels: &["Work", "Legal"],
        turns: &[
            (
                false,
                "Alex,\n\nAttached are our redlines on the services agreement. The main changes are in §4 (IP assignment) and §9 (limitation of liability). Happy to walk through them on a call.\n\nElena Petrova\nPetrova Law",
            ),
            (
                true,
                "Thanks Elena. §4 looks fine. On §9 we'd like the cap at 12 months of fees rather than 6. Can you propose that?",
            ),
        ],
        attachment: Some(("northwind-msa-redlines.pdf", "application/pdf")),
    },
    Script {
        subject: "Climbing gym membership renewal",
        labels: &["Receipts"],
        turns: &[(
            false,
            "Your annual membership at Mission Cliffs has been renewed.\n\nPlan: Annual Unlimited\nAmount: $1,020.00\nNext renewal: next year\n\nSee you on the wall!",
        )],
        attachment: None,
    },
    Script {
        subject: "Code review: incremental FTS backfill",
        labels: &["Work", "Projects/Atlas"],
        turns: &[
            (
                false,
                "Left a few comments on the PR. The batching looks right, but I think we need a checkpoint table so a crash mid-backfill doesn't restart from zero.",
            ),
            (
                true,
                "Good catch. Added `fts_checkpoints` with the last rowid per batch; resuming picks up from there. Mind taking another look?",
            ),
            (false, "LGTM now 🚢 Nice work."),
        ],
        attachment: None,
    },
    Script {
        subject: "Apartment lease renewal",
        labels: &["Home"],
        turns: &[
            (
                false,
                "Hi Alex, your lease ends on the 30th of next month. We'd love to have you stay! The renewal rate for a 12-month term is attached. Please let us know by the 15th.",
            ),
            (
                true,
                "Thanks! I'd like to renew for 12 months. Is there any flexibility on the rate if I sign this week?",
            ),
        ],
        attachment: Some(("lease-renewal-offer.pdf", "application/pdf")),
    },
    Script {
        subject: "Conference talk accepted: \"Local-first archives\"",
        labels: &["Work", "Travel"],
        turns: &[(
            false,
            "<p>Dear Alex,</p><p>We're delighted to let you know that your talk <b>Local-first archives: owning your communication history</b> has been accepted for <i>StrangeLoop Revival</i>.</p><ul><li>Slot: 30 minutes + 10 minutes Q&amp;A</li><li>Speaker dinner: the evening before</li><li>Travel and hotel covered</li></ul><p>Please confirm by replying to this email.</p><p>— The Program Committee</p>",
        )],
        attachment: None,
    },
    Script {
        subject: "Hiking this weekend?",
        labels: &[],
        turns: &[
            (
                false,
                "Weather looks perfect for Mount Tam on Sunday. Early start, 7am at the Pantoll lot?",
            ),
            (true, "I'm in. I'll bring coffee."),
            (false, "Legend. See you there."),
        ],
        attachment: None,
    },
];

const STUDIO_SCRIPTS: &[Script] = &[
    Script {
        subject: "Kickoff: Fjord Outdoor brand refresh",
        labels: &[],
        turns: &[
            (
                false,
                "Hi Alex,\n\nExcited to get started! Attached is our brief. The big goals: modernise the wordmark, build a flexible color system for seasonal campaigns, and make the site feel as calm as a walk in the mountains.\n\nCould we do a kickoff call Tuesday or Wednesday?\n\nIngrid",
            ),
            (
                true,
                "Hi Ingrid, thanks for the thorough brief. Wednesday 10:00 CET works. I'll send an invite with a short agenda.",
            ),
        ],
        attachment: Some(("fjord-brand-brief.pdf", "application/pdf")),
    },
    Script {
        subject: "Website launch checklist",
        labels: &[],
        turns: &[
            (
                true,
                "Team, launch is Monday. Remaining items:\n\n- Final copy review (Kenji)\n- 301 redirects from the old domain (me)\n- Analytics + consent banner (Diego)\n- Social cards for each case study (Mara)\n\nShout if anything is blocked.",
            ),
            (false, "Copy review done, two small fixes pushed. 👍"),
            (
                false,
                "Consent banner is live on staging. Analytics events verified.",
            ),
        ],
        attachment: None,
    },
    Script {
        subject: "Quote request: packaging design for Watanabe Tea",
        labels: &[],
        turns: &[(
            false,
            "<p>Hello Morgan Studio,</p><p>We're a small tea company in Shizuoka looking for packaging design for a new line of <b>four seasonal blends</b>. We'd need:</p><ul><li>Tin label system (4 variants)</li><li>Outer carton</li><li>A simple product sheet</li></ul><p>Could you share a rough quote and timeline?</p><p>Best regards,<br>Kenji Watanabe</p>",
        )],
        attachment: None,
    },
    Script {
        subject: "Studio retreat — venue options",
        labels: &[],
        turns: &[
            (
                false,
                "Found three options for the October retreat: a farmhouse in Sonoma, a cabin cluster near Tahoe, and a surf lodge in Pacifica. Pros/cons in the sheet!",
            ),
            (
                true,
                "The Sonoma farmhouse looks perfect. Can you check availability for the 17th–19th?",
            ),
        ],
        attachment: Some(("retreat-venues.csv", "text/csv")),
    },
    Script {
        subject: "Payment received — thank you!",
        labels: &[],
        turns: &[(
            false,
            "Hi Alex, just confirming we've received payment for invoice MS-0142. Thanks for the fantastic work on the annual report — the board loved it.\n\nJonas",
        )],
        attachment: None,
    },
];

const WHATSAPP_GROUP: &[(usize, &str)] = &[
    (5, "Who's in for bouldering Thursday? 🧗"),
    (99, "Me! After 6 works best"),
    (7, "Count me in, need to work on my heel hooks"),
    (5, "Booked a 6:30 slot for 3"),
    (99, "🔥🔥"),
    (7, "Anyone want to try the new V5 in the cave?"),
    (5, "Only if you promise not to laugh at me again"),
    (99, "No promises 😂"),
    (7, "Tacos after?"),
    (5, "Always tacos after"),
];

const WHATSAPP_DM: &[(usize, &str)] = &[
    (6, "Did you get home ok?"),
    (99, "Yep! Thanks again for dinner, that risotto was unreal"),
    (6, "Haha I'll send you the recipe"),
    (6, "It's mostly butter tbh"),
    (99, "The best recipes are"),
];

const SLACK_CHANNEL: &[(usize, &str)] = &[
    (
        2,
        "Posted the new onboarding flow in Figma — feedback welcome before Friday",
    ),
    (
        99,
        "Looks great. The empty state on step 3 feels a bit bare, maybe an illustration?",
    ),
    (
        2,
        "Good call, I'll try one of the spot illustrations from the brand kit",
    ),
    (1, "Can we make the progress indicator sticky on mobile?"),
    (2, "Yes, updated ✅"),
    (99, "Ship it 🚀"),
];

const EVENTS: &[(&str, &str)] = &[
    (
        "Leadership sync",
        "Weekly sync with the leadership team. Agenda in the shared doc.",
    ),
    (
        "1:1 with Priya",
        "Career growth, Atlas launch, hiring plan.",
    ),
    ("Dentist", "Dr. Kim — cleaning. 2200 Mission St."),
    ("Design crit", "Onboarding flow v3 walkthrough."),
    ("Flight TP 202 to Lisbon", "SFO → LIS. Confirmation QX7LMP."),
];

struct Builder<'c> {
    conn: &'c Connection,
    root: std::path::PathBuf,
    now: DateTime<Utc>,
    rng: Rng,
    participants: HashMap<String, i64>,
    labels: HashMap<(i64, String), i64>,
    pack: Vec<(String, Vec<u8>, bool)>,
    summary: FixtureSummary,
    uid: i64,
}

/// A message to insert; built by the generators below.
#[derive(Default)]
struct Draft {
    conversation: i64,
    source: i64,
    kind: &'static str,
    subject: Option<String>,
    from: (String, String),
    to: Vec<(String, String)>,
    cc: Vec<(String, String)>,
    sent_at: Option<DateTime<Utc>>,
    text: Option<String>,
    html: Option<String>,
    from_me: bool,
    labels: Vec<String>,
    removed_upstream: bool,
    /// Store only the raw MIME, as older archives did.
    raw_only: bool,
}

fn ts(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S%:z").to_string()
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            },
            c if !in_tag => out.push(c),
            _ => {},
        }
    }
    out.replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Builder<'_> {
    fn populate(&mut self) -> Result<()> {
        let gmail = self.source("gmail", ME_GMAIL.email, "Alex Morgan (Gmail)")?;
        let work = self.source("imap", ME_WORK.email, "Morgan Studio")?;
        let whatsapp = self.source("whatsapp", "+14155550123", "WhatsApp")?;
        let slack = self.source("slack", "morgan-studio.slack.com", "Morgan Studio Slack")?;
        let calendar = self.source("gcal", ME_GMAIL.email, "Google Calendar")?;
        self.summary.accounts = 5;

        for (name, color) in [
            ("Work", "#4a86e8"),
            ("Projects/Atlas", "#8e63ce"),
            ("Travel", "#16a766"),
            ("Receipts", "#f2a60c"),
            ("Family", "#e66550"),
            ("Legal", "#a479e2"),
            ("Home", "#3dc789"),
        ] {
            self.label(gmail, name, "user", Some(color))?;
        }

        // Gmail: every script twice with different people and times, newest first.
        let mut minutes = 35;
        for round in 0..3 {
            for (index, script) in SCRIPTS.iter().enumerate() {
                if round == 2 && index % 3 != 0 {
                    continue;
                }
                let person = PEOPLE[(index * 5 + round * 3) % PEOPLE.len()];
                self.email_thread(gmail, ME_GMAIL, person, script, minutes, round, true)?;
                minutes += 190 + self.rng.below(900) as i64;
            }
            for (index, sender) in NEWSLETTERS.iter().enumerate() {
                self.newsletter(gmail, sender, round * NEWSLETTERS.len() + index, minutes)?;
                minutes += 240 + self.rng.below(600) as i64;
            }
        }

        // Work IMAP account: folders become labels, state lives in flags.
        let mut minutes = 80;
        for (index, script) in STUDIO_SCRIPTS.iter().enumerate() {
            let person = [PEOPLE[8], PEOPLE[5], PEOPLE[9], PEOPLE[2], PEOPLE[1]][index % 5];
            self.email_thread(work, ME_WORK, person, script, minutes, 0, false)?;
            minutes += 420 + self.rng.below(1200) as i64;
        }

        self.chat(
            whatsapp,
            "whatsapp",
            "group_chat",
            "Weekend Climbing 🧗",
            WHATSAPP_GROUP,
            50,
        )?;
        self.chat(
            whatsapp,
            "whatsapp",
            "direct_chat",
            "Hannah Lee",
            WHATSAPP_DM,
            600,
        )?;
        self.chat(
            slack,
            "slack",
            "channel",
            "#design-review",
            SLACK_CHANNEL,
            200,
        )?;
        self.calendar(calendar)?;
        Ok(())
    }

    fn source(&mut self, kind: &str, identifier: &str, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sources (source_type, identifier, display_name, last_sync_at) VALUES (?1, ?2, ?3, ?4)",
            params![kind, identifier, name, ts(self.now - Duration::minutes(12))],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn label(&mut self, source: i64, name: &str, kind: &str, color: Option<&str>) -> Result<i64> {
        if let Some(id) = self.labels.get(&(source, name.to_owned())) {
            return Ok(*id);
        }
        let role = (name == "SENT" || name == "Sent").then_some("sent");
        self.conn.execute(
            "INSERT INTO labels (source_id, source_label_id, name, label_type, system_role, color)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
            params![source, name, kind, role, color],
        )?;
        let id = self.conn.last_insert_rowid();
        self.labels.insert((source, name.to_owned()), id);
        Ok(id)
    }

    fn participant(&mut self, name: &str, address: &str) -> Result<i64> {
        if let Some(id) = self.participants.get(address) {
            return Ok(*id);
        }
        let is_phone = address.starts_with('+');
        let domain = address.split_once('@').map(|(_, d)| d.to_owned());
        self.conn.execute(
            "INSERT INTO participants (email_address, phone_number, display_name, domain) VALUES (?1, ?2, ?3, ?4)",
            params![
                (!is_phone).then_some(address),
                is_phone.then_some(address),
                name,
                domain
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.participants.insert(address.to_owned(), id);
        Ok(id)
    }

    fn conversation(&mut self, source: i64, kind: &str, title: &str) -> Result<i64> {
        let key = format!("conv-{}", self.uid);
        self.uid += 1;
        self.conn.execute(
            "INSERT INTO conversations (source_id, source_conversation_id, conversation_type, title) VALUES (?1, ?2, ?3, ?4)",
            params![source, key, kind, title],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn email_thread(
        &mut self,
        source: i64,
        me: Person,
        person: Person,
        script: &Script,
        minutes_ago: i64,
        round: usize,
        gmail: bool,
    ) -> Result<()> {
        let subject = if round == 0 {
            script.subject.to_owned()
        } else {
            format!(
                "{} ({})",
                script.subject,
                ["", "follow-up", "archive"][round.min(2)]
            )
        };
        let conversation = self.conversation(source, "email_thread", &subject)?;
        let turns = script.turns.len() as i64;
        // The newest IMAP thread is always unread, so \Seen handling stays covered.
        let unread = (!gmail && minutes_ago < 500)
            || self.rng.chance(if round == 0 {
                45
            } else {
                15
            });
        let starred = self.rng.chance(18);
        let important = self.rng.chance(30);
        for (index, (from_me, body)) in script.turns.iter().enumerate() {
            let last = index as i64 == turns - 1;
            let sent_at = self.now
                - Duration::minutes(
                    minutes_ago + (turns - 1 - index as i64) * (47 + round as i64 * 31),
                );
            let (from, to) = if *from_me {
                (me, person)
            } else {
                (person, me)
            };
            let is_html = body.starts_with('<');
            let mut labels: Vec<String> = Vec::new();
            if gmail {
                labels.push(
                    if *from_me {
                        "SENT"
                    } else {
                        "INBOX"
                    }
                    .into(),
                );
                if last && unread && !from_me {
                    labels.push("UNREAD".into());
                }
                if last && starred {
                    labels.push("STARRED".into());
                }
                if important {
                    labels.push("IMPORTANT".into());
                }
                labels.extend(script.labels.iter().map(|l| (*l).to_owned()));
                if round == 2 {
                    labels.retain(|l| l != "INBOX");
                }
            } else {
                labels.push(
                    if *from_me {
                        "Sent"
                    } else {
                        "INBOX"
                    }
                    .into(),
                );
            }
            let cc = if index == 0 && self.rng.chance(30) {
                let other = self.rng.pick(PEOPLE);
                vec![(other.name.to_owned(), other.email.to_owned())]
            } else {
                Vec::new()
            };
            let draft = Draft {
                conversation,
                source,
                kind: "email",
                subject: Some(if index == 0 || subject.starts_with("Re:") {
                    subject.clone()
                } else {
                    format!("Re: {subject}")
                }),
                from: (from.name.to_owned(), from.email.to_owned()),
                to: vec![(to.name.to_owned(), to.email.to_owned())],
                cc,
                sent_at: Some(sent_at),
                text: (!is_html).then(|| (*body).to_owned()),
                html: is_html.then(|| (*body).to_owned()),
                from_me: *from_me,
                labels,
                removed_upstream: round == 2 && index == 0 && self.rng.chance(50),
                raw_only: gmail && round == 1 && script.subject.starts_with("Hiking"),
            };
            let id = self.message(draft)?;
            if !gmail {
                let flags = if *from_me || !(last && unread) {
                    if last && starred {
                        r#"["\\Seen","\\Flagged"]"#
                    } else {
                        r#"["\\Seen"]"#
                    }
                } else {
                    "[]"
                };
                let mailbox = if *from_me {
                    "Sent"
                } else {
                    "INBOX"
                };
                self.conn.execute(
                    "INSERT INTO imap_message_memberships (source_id, mailbox, uidvalidity, uid, message_id, flags)
                     VALUES (?1, ?2, 1, ?3, ?4, ?5)",
                    params![source, mailbox, id, id, flags],
                )?;
            }
            if last && let Some((filename, mime)) = script.attachment {
                let packed = self.summary.attachments % 3 == 1;
                self.attachment(id, filename, mime, &subject, packed)?;
            }
        }
        Ok(())
    }

    fn newsletter(
        &mut self,
        source: i64,
        sender: &Sender,
        issue: usize,
        minutes_ago: i64,
    ) -> Result<()> {
        let subject = match sender.category {
            "CATEGORY_PROMOTIONS" => format!("{}: up to 40% off — this weekend only", sender.name),
            "CATEGORY_SOCIAL" => format!("{}: your weekly recap is here", sender.name),
            _ => format!("{} — issue #{}", sender.name, 180 + issue),
        };
        let html = format!(
            "<h1>{name}</h1><p>Hi Alex, here's what's new this week.</p>\
             <h3>Highlights</h3><ul><li>A deep dive into <b>zero-copy parsing</b></li>\
             <li>Five tools we couldn't live without</li><li>Community spotlight: local-first software</li></ul>\
             <blockquote>“The best archive is the one you own.”</blockquote>\
             <p><a href=\"https://example.com/read\">Read the full issue →</a></p>\
             <p><small>You're receiving this because you subscribed. <a href=\"https://example.com/unsubscribe\">Unsubscribe</a></small></p>",
            name = sender.name
        );
        let conversation = self.conversation(source, "email_thread", &subject)?;
        let mut labels = vec!["INBOX".to_owned(), sender.category.to_owned()];
        if self.rng.chance(55) {
            labels.push("UNREAD".into());
        }
        self.message(Draft {
            conversation,
            source,
            kind: "email",
            subject: Some(subject),
            from: (sender.name.to_owned(), sender.email.to_owned()),
            to: vec![(ME_GMAIL.name.to_owned(), ME_GMAIL.email.to_owned())],
            sent_at: Some(self.now - Duration::minutes(minutes_ago)),
            html: Some(html),
            labels,
            ..Draft::default()
        })?;
        Ok(())
    }

    fn chat(
        &mut self,
        source: i64,
        kind: &'static str,
        conversation_type: &str,
        title: &str,
        lines: &[(usize, &str)],
        minutes_ago: i64,
    ) -> Result<()> {
        let conversation = self.conversation(source, conversation_type, title)?;
        let count = lines.len() as i64;
        for (index, (who, text)) in lines.iter().enumerate() {
            let (name, address) = match PEOPLE.get(*who) {
                Some(person) if kind == "slack" => {
                    (person.name.to_owned(), person.email.to_owned())
                },
                Some(person) => (person.name.to_owned(), format!("+1415555{:04}", 1000 + who)),
                None if kind == "slack" => (ME_WORK.name.to_owned(), ME_WORK.email.to_owned()),
                None => (ME_GMAIL.name.to_owned(), "+14155550123".to_owned()),
            };
            self.message(Draft {
                conversation,
                source,
                kind,
                from: (name, address),
                sent_at: Some(
                    self.now - Duration::minutes(minutes_ago + (count - index as i64) * 6),
                ),
                text: Some((*text).to_owned()),
                from_me: *who == 99,
                ..Draft::default()
            })?;
        }
        Ok(())
    }

    fn calendar(&mut self, source: i64) -> Result<()> {
        for (index, (title, description)) in EVENTS.iter().enumerate() {
            let conversation = self.conversation(source, "calendar", title)?;
            self.message(Draft {
                conversation,
                source,
                kind: "calendar_event",
                subject: Some((*title).to_owned()),
                from: (ME_GMAIL.name.to_owned(), ME_GMAIL.email.to_owned()),
                sent_at: Some(self.now - Duration::hours(20 + index as i64 * 31)),
                text: Some((*description).to_owned()),
                from_me: true,
                ..Draft::default()
            })?;
        }
        Ok(())
    }

    fn message(&mut self, draft: Draft) -> Result<i64> {
        let sender = self.participant(&draft.from.0, &draft.from.1)?;
        let body = draft
            .text
            .clone()
            .or_else(|| draft.html.as_deref().map(strip_html))
            .unwrap_or_default();
        let snippet: String = body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(140)
            .collect();
        let source_message_id = format!("msg-{}", self.uid);
        self.uid += 1;
        let sent = draft.sent_at.map(ts);
        self.conn.execute(
            "INSERT INTO messages (conversation_id, source_id, source_message_id, message_type,
                sent_at, received_at, internal_date, sender_id, is_from_me, subject, snippet,
                size_estimate, deleted_from_source_at, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1)",
            params![
                draft.conversation,
                draft.source,
                source_message_id,
                draft.kind,
                sent,
                sender,
                draft.from_me,
                draft.subject,
                snippet,
                (body.len() + 900) as i64,
                draft
                    .removed_upstream
                    .then(|| ts(self.now - Duration::days(3))),
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.summary.messages += 1;

        self.conn.execute(
            "INSERT INTO message_recipients (message_id, participant_id, recipient_type, display_name, email_address)
             VALUES (?1, ?2, 'from', ?3, ?4)",
            params![id, sender, draft.from.0, draft.from.1],
        )?;
        for (kind, list) in [("to", &draft.to), ("cc", &draft.cc)] {
            for (name, address) in list {
                let participant = self.participant(name, address)?;
                self.conn.execute(
                    "INSERT INTO message_recipients (message_id, participant_id, recipient_type, display_name, email_address)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, participant, kind, name, address],
                )?;
            }
        }
        for name in &draft.labels {
            let kind = if crate::model::SYSTEM_LABELS.contains(&name.to_ascii_uppercase().as_str())
                || name.starts_with("CATEGORY_")
            {
                "system"
            } else {
                "user"
            };
            let label = self.label(draft.source, name, kind, None)?;
            self.conn.execute(
                "INSERT OR IGNORE INTO message_labels (message_id, label_id) VALUES (?1, ?2)",
                params![id, label],
            )?;
        }
        if draft.raw_only {
            self.raw_mime(id, &draft)?;
        } else {
            self.conn.execute(
                "INSERT INTO message_bodies (message_id, body_text, body_html) VALUES (?1, ?2, ?3)",
                params![id, draft.text, draft.html],
            )?;
        }
        let to = draft
            .to
            .iter()
            .map(|(_, a)| a.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let cc = draft
            .cc
            .iter()
            .map(|(_, a)| a.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.conn.execute(
            "INSERT INTO messages_fts (rowid, message_id, subject, body, from_addr, to_addr, cc_addr)
             VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, draft.subject.as_deref().unwrap_or_default(), body, draft.from.1, to, cc],
        )?;
        self.conn.execute(
            "UPDATE conversations SET message_count = message_count + 1,
                last_message_at = MAX(COALESCE(last_message_at, ''), ?2), last_message_preview = ?3
             WHERE id = ?1",
            params![draft.conversation, sent, snippet],
        )?;
        Ok(id)
    }

    fn raw_mime(&mut self, id: i64, draft: &Draft) -> Result<()> {
        let mime = format!(
            "From: {} <{}>\r\nTo: {}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
            draft.from.0,
            draft.from.1,
            draft
                .to
                .iter()
                .map(|(n, a)| format!("{n} <{a}>"))
                .collect::<Vec<_>>()
                .join(", "),
            draft.subject.as_deref().unwrap_or_default(),
            draft.text.as_deref().unwrap_or_default()
        );
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(mime.as_bytes())
            .map_err(Error::Decompress)?;
        let compressed = encoder.finish().map_err(Error::Decompress)?;
        self.conn.execute(
            "INSERT INTO message_raw (message_id, raw_data, raw_format, compression) VALUES (?1, ?2, 'mime', 'zlib')",
            params![id, compressed],
        )?;
        Ok(())
    }

    fn attachment(
        &mut self,
        message: i64,
        filename: &str,
        mime: &str,
        subject: &str,
        packed: bool,
    ) -> Result<()> {
        let content = attachment_content(filename, mime, subject);
        let hash = hex(&Sha256::digest(&content));
        let storage_path = format!("{}/{hash}", &hash[..2]);
        if packed {
            self.pack.push((hash.clone(), content.clone(), true));
        } else {
            let path = self.root.join(&storage_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            std::fs::write(&path, &content).map_err(|e| Error::io(&path, e))?;
        }
        self.conn.execute(
            "INSERT INTO attachments (message_id, filename, mime_type, size, content_hash, storage_path,
                media_type, attachment_state, attachment_role, role_source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'stored', 'standalone', 'mime_disposition')",
            params![
                message,
                filename,
                mime,
                content.len() as i64,
                hash,
                storage_path,
                if mime.starts_with("image/") { "image" } else { "document" }
            ],
        )?;
        self.conn.execute(
            "UPDATE messages SET has_attachments = 1, attachment_count = attachment_count + 1 WHERE id = ?1",
            [message],
        )?;
        self.summary.attachments += 1;
        Ok(())
    }

    /// Seals queued blobs into one `.mvpack` (header, then blobs; the index
    /// rows carry the offsets, which is all a reader needs).
    fn finish_pack(&mut self) -> Result<()> {
        if self.pack.is_empty() {
            return Ok(());
        }
        let mut file = b"MVPK\x01\x00".to_vec();
        for (hash, content, compress) in std::mem::take(&mut self.pack) {
            let stored = if compress {
                zstd::encode_all(content.as_slice(), 3).map_err(Error::Decompress)?
            } else {
                content.clone()
            };
            let exists: Option<i64> = self
                .conn
                .query_row(
                    "SELECT 1 FROM attachment_pack_index WHERE blob_hash = ?1",
                    [&hash],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_some() {
                continue;
            }
            self.conn.execute(
                "INSERT INTO attachment_pack_index (blob_hash, pack_id, pack_offset, stored_len, raw_len, flags, crc32c)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                params![hash, PACK_ID, file.len() as i64, stored.len() as i64, content.len() as i64, i64::from(compress)],
            )?;
            file.extend_from_slice(&stored);
        }
        let dir = self.root.join("packs").join(&PACK_ID[..2]);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let path = dir.join(format!("{PACK_ID}.mvpack"));
        std::fs::write(&path, file).map_err(|e| Error::io(&path, e))?;
        Ok(())
    }
}

/// Small but real file contents, so previews open in native apps.
fn attachment_content(filename: &str, mime: &str, subject: &str) -> Vec<u8> {
    match mime {
        "application/pdf" => minimal_pdf(subject),
        "text/calendar" => format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//msgvault-gpui//fixture//EN\r\nBEGIN:VEVENT\r\nUID:{filename}\r\nSUMMARY:{subject}\r\nDTSTART:20240920T184500Z\r\nDTEND:20240921T140000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        )
        .into_bytes(),
        "image/png" => gradient_png(320, 200),
        _ => format!("{subject}\n").into_bytes(),
    }
}

fn minimal_pdf(title: &str) -> Vec<u8> {
    let text: String = title
        .chars()
        .filter(|c| c.is_ascii() && *c != '(' && *c != ')' && *c != '\\')
        .collect();
    let stream = format!("BT /F1 18 Tf 72 720 Td ({text}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A small gradient PNG, encoded by hand so no image crate is needed.
fn gradient_png(width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            let t = x as f32 / width as f32;
            let u = y as f32 / height as f32;
            raw.extend_from_slice(&[
                (40.0 + 180.0 * t) as u8,
                (90.0 + 100.0 * u) as u8,
                (200.0 - 90.0 * t) as u8,
            ]);
        }
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let idat = encoder
        .write_all(&raw)
        .and_then(|()| encoder.finish())
        .unwrap_or_default();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", ihdr), (b"IDAT", idat), (b"IEND", Vec::new())] {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(&data);
        out.extend_from_slice(kind);
        out.extend_from_slice(&data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    }
    out
}
