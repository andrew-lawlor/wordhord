//! The hoard: words kept from the dictionary panel, each with the sentence
//! it was met in. A log, `hoard.jsonl` in the app folder, one record a
//! line: a word kept, or a keeping let go. Never edited, only added to, so
//! two copies merge and nothing is lost.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wordhord_book::model::{self, Block};

use crate::fsrs;

/// A word kept, with where it was met.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub id: String,
    /// Seconds since 1970.
    pub time: u64,
    /// As printed: "forc'd".
    pub word: String,
    /// The dictionary's form: "force".
    pub head: String,
    /// The dictionary pack it was found in: "eng", "non".
    pub lang: String,
    pub pos: String,
    pub sentence: String,
    /// Where the word is in the sentence, in characters.
    pub at: usize,
    pub book: String,
    pub path: String,
    pub chapter: usize,
    pub block: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Keep(Box<Kept>),
    Drop {
        id: String,
        time: u64,
    },
    /// A word unlocked: graded 1 to 4, again to easy.
    Review(Review),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub head: String,
    pub lang: String,
    pub grade: u8,
    pub time: u64,
}

/// A word in the hoard, and every time it was kept, newest first.
pub struct Word<'a> {
    pub head: &'a str,
    pub lang: &'a str,
    pub pos: &'a str,
    pub kept: Vec<&'a Kept>,
}

/// A word's stability, in days, from which it's learned.
pub const LEARNED_DAYS: f32 = 21.0;

/// Words to unlock today, at most this many new.
pub const NEW_A_DAY: usize = 10;

pub struct Hoard {
    path: PathBuf,
    /// Kept and not let go, oldest first.
    kept: Vec<Kept>,
    /// Every review, oldest first.
    reviews: Vec<Review>,
    /// How many words have been kept, let go or unlocked: so what's built
    /// from the hoard knows when to build again.
    changes: usize,
}

impl Hoard {
    pub fn load(app: &Path) -> Hoard {
        let path = app.join("hoard.jsonl");
        let mut kept: Vec<Kept> = Vec::new();
        let mut reviews = Vec::new();
        let mut changes = 0;
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            // A line cut short by a crash is skipped, not fatal.
            match serde_json::from_str::<Record>(line) {
                Ok(Record::Keep(k)) => {
                    kept.push(*k);
                    changes += 1;
                }
                Ok(Record::Drop { id, .. }) => {
                    kept.retain(|k| k.id != id);
                    changes += 1;
                }
                Ok(Record::Review(r)) => reviews.push(r),
                Err(_) => {}
            }
        }
        // Two copies merged may interleave: replay in time order.
        reviews.sort_by_key(|r: &Review| r.time);
        Hoard {
            path,
            kept,
            reviews,
            changes,
        }
    }

    fn append(&self, record: &Record) -> Result<(), String> {
        let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|e| format!("{}: {e}", self.path.display()))
    }

    pub fn keep(&mut self, mut k: Kept) -> Result<(), String> {
        let time = now();
        k.time = time;
        k.id = format!("{time:x}-{:x}", nanos());
        self.append(&Record::Keep(Box::new(k.clone())))?;
        self.kept.push(k);
        self.changes += 1;
        Ok(())
    }

    pub fn let_go(&mut self, id: &str) -> Result<(), String> {
        self.append(&Record::Drop {
            id: id.to_string(),
            time: now(),
        })?;
        self.kept.retain(|k| k.id != id);
        self.changes += 1;
        Ok(())
    }

    pub fn review(&mut self, head: &str, lang: &str, grade: u8) -> Result<(), String> {
        let r = Review {
            head: head.to_string(),
            lang: lang.to_string(),
            grade,
            time: now(),
        };
        self.append(&Record::Review(r.clone()))?;
        self.reviews.push(r);
        self.changes += 1;
        Ok(())
    }

    /// The word's reviews, oldest first.
    fn reviews_of<'a>(&'a self, head: &'a str, lang: &'a str) -> impl Iterator<Item = &'a Review> {
        self.reviews
            .iter()
            .filter(move |r| r.head == head && r.lang == lang)
    }

    /// How well the word is held, from its reviews; `None` if never
    /// reviewed.
    /// Whether a word is learned: recalled nine times in ten for three
    /// weeks or more, as Anki calls a card mature. Forgotten, it isn't
    /// again.
    pub fn learned(&self, head: &str, lang: &str) -> bool {
        self.state(head, lang)
            .is_some_and(|s| s.stability >= LEARNED_DAYS)
    }

    pub fn state(&self, head: &str, lang: &str) -> Option<fsrs::State> {
        self.reviews_of(head, lang)
            .fold(None, |s, r| Some(fsrs::next(s, r.grade, r.time)))
    }

    /// How many times the word has been reviewed.
    pub fn reviewed(&self, head: &str, lang: &str) -> usize {
        self.reviews_of(head, lang).count()
    }

    /// The words to unlock at `time`, as (head, lang): those due, the
    /// longest overdue first; then new words, the first kept first, so
    /// that no more than `NEW_A_DAY` are started in a day.
    pub fn due(&self, time: u64) -> Vec<(String, String)> {
        let today = time / fsrs::DAY;
        let mut old = Vec::new();
        let mut new = Vec::new();
        let mut started_today = 0;
        for w in self.words().iter().rev() {
            match self.state(w.head, w.lang) {
                Some(s) => {
                    let first = self.reviews_of(w.head, w.lang).next().map_or(0, |r| r.time);
                    if first / fsrs::DAY == today {
                        started_today += 1;
                    }
                    if fsrs::due_day(&s) <= today {
                        old.push((fsrs::due_day(&s), w.head.to_string(), w.lang.to_string()));
                    }
                }
                None => new.push((w.head.to_string(), w.lang.to_string())),
            }
        }
        old.sort_by_key(|o| o.0);
        old.into_iter()
            .map(|(_, h, l)| (h, l))
            .chain(
                new.into_iter()
                    .take(NEW_A_DAY.saturating_sub(started_today)),
            )
            .collect()
    }

    /// What was kept of the word printed so in this paragraph or stanza,
    /// under whichever reading. The word may be there more than once:
    /// `sentence` and `at` tell which.
    pub fn kept_in(&self, word: &str, path: &str, chapter: usize, block: &str) -> Vec<&Kept> {
        let key = wordhord_dict::key(word);
        self.kept
            .iter()
            .filter(|k| {
                k.path == path
                    && k.chapter == chapter
                    && k.block == block
                    && wordhord_dict::key(&k.word) == key
            })
            .collect()
    }

    pub fn changes(&self) -> usize {
        self.changes
    }

    /// Where a word, as one reading, was first kept.
    pub fn first_met(&self, head: &str, pos: &str, lang: &str) -> Option<&Kept> {
        self.kept
            .iter()
            .filter(|k| k.head == head && k.pos == pos && k.lang == lang)
            .min_by_key(|k| k.time)
    }

    /// The words, each once, the latest kept first.
    pub fn words(&self) -> Vec<Word<'_>> {
        let mut words: Vec<Word> = Vec::new();
        for k in self.kept.iter().rev() {
            match words
                .iter_mut()
                .find(|w| w.head == k.head && w.lang == k.lang)
            {
                Some(w) => w.kept.push(k),
                None => words.push(Word {
                    head: &k.head,
                    lang: &k.lang,
                    pos: &k.pos,
                    kept: vec![k],
                }),
            }
        }
        words
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn nanos() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
}

/// The sentence around the `nth` word of the block `id` (counting as the
/// pages do, by spaces), and where that word is in it. If the count lands
/// on another word (a note marker counted differently), the nearest
/// `word` is taken instead. Lines of verse are joined with " / ".
pub fn sentence(blocks: &[Block], id: &str, nth: usize, word: &str) -> Option<(String, usize)> {
    let mut all = Vec::new();
    model::walk(blocks, &mut all);
    let block = all.into_iter().find(|b| b.id.as_deref() == Some(id))?;
    // Each word, and whether a line of verse ends after it.
    let mut tokens: Vec<(String, bool)> = Vec::new();
    for line in model::block_inlines(block) {
        let text = model::plain_text(line);
        let before = tokens.len();
        tokens.extend(text.split_whitespace().map(|t| (t.to_string(), false)));
        if tokens.len() > before {
            if let Some(last) = tokens.last_mut() {
                last.1 = true;
            }
        }
    }
    if tokens.is_empty() {
        return None;
    }
    let key = wordhord_dict::key(word);
    let same = |t: &str| wordhord_dict::key(t) == key;
    let at = if tokens.get(nth).is_some_and(|t| same(&t.0)) {
        nth
    } else {
        (0..tokens.len())
            .filter(|&k| same(&tokens[k].0))
            .min_by_key(|&k| k.abs_diff(nth))
            .unwrap_or(nth.min(tokens.len() - 1))
    };
    let ends = |t: &str| {
        t.trim_end_matches(['"', '\'', '’', '”', ')', ']', '»'])
            .ends_with(['.', '!', '?'])
    };
    // Back to the end of the sentence before, forward to this one's end;
    // forty words at most each way.
    let mut start = at;
    while start > 0 && at - start < 40 && !ends(&tokens[start - 1].0) {
        start -= 1;
    }
    let mut end = at;
    while end + 1 < tokens.len() && end - at < 40 && !ends(&tokens[end].0) {
        end += 1;
    }
    let mut text = String::new();
    let mut offset = 0;
    if start > 0 && !ends(&tokens[start - 1].0) {
        text.push_str("… ");
    }
    for (k, (t, line_end)) in tokens.iter().enumerate().take(end + 1).skip(start) {
        if k == at {
            offset = text.chars().count();
        }
        text.push_str(t);
        if k < end {
            // A verse line ends: mark it, as a quotation of verse does.
            let verse = *line_end && block_inlines_are_lines(block);
            text.push_str(if verse { " / " } else { " " });
        }
    }
    if end + 1 < tokens.len() && !ends(&tokens[end].0) {
        text.push_str(" …");
    }
    Some((text, offset))
}

/// Whether the block `id` is verse, set line by line.
pub fn is_verse(blocks: &[Block], id: &str) -> bool {
    let mut all = Vec::new();
    model::walk(blocks, &mut all);
    all.into_iter()
        .find(|b| b.id.as_deref() == Some(id))
        .is_some_and(block_inlines_are_lines)
}

fn block_inlines_are_lines(b: &Block) -> bool {
    use wordhord_book::model::BlockKind;
    matches!(b.kind, BlockKind::Verse(_) | BlockKind::Stanza(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordhord_book::model::{BlockKind, Inline, Verse, VerseLine};

    fn para(id: &str, text: &str) -> Block {
        Block {
            id: Some(id.into()),
            kind: BlockKind::Paragraph(vec![Inline::Text(text.into())]),
            lang: None,
            reference: None,
            line: 0,
        }
    }

    #[test]
    fn a_word_is_learned_at_three_weeks_and_unlearned_when_forgotten() {
        let dir = std::env::temp_dir().join(format!("wordhord-learned-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let day = 86_400;
        let review = |grade: u8, time: u64| {
            serde_json::to_string(&Record::Review(Review {
                head: "hate".into(),
                lang: "eng".into(),
                grade,
                time,
            }))
            .unwrap()
        };
        let log = |lines: &[String]| {
            std::fs::write(dir.join("hoard.jsonl"), lines.join("\n")).unwrap();
            Hoard::load(&dir)
        };
        let mut lines = vec![review(3, 0)];
        assert!(!log(&lines).learned("hate", "eng"));
        lines.extend([review(3, 4 * day), review(4, 20 * day)]);
        assert!(log(&lines).learned("hate", "eng"));
        lines.push(review(1, 90 * day));
        assert!(!log(&lines).learned("hate", "eng"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_sentence_around_a_word() {
        let blocks = [para(
            "b1",
            "It rained. The man I sing, forc'd by fate, came home! Then he slept.",
        )];
        let (s, at) = sentence(&blocks, "b1", 6, "forc'd").unwrap();
        assert_eq!(s, "The man I sing, forc'd by fate, came home!");
        assert_eq!(&s.chars().skip(at).collect::<String>()[..6], "forc'd");
    }

    #[test]
    fn verse_lines_are_joined_with_a_slash() {
        let line = |t: &str| VerseLine {
            indent: 0,
            group: 0,
            inlines: vec![Inline::Text(t.into())],
            reference: None,
            line: 0,
        };
        let blocks = [Block {
            id: Some("v".into()),
            kind: BlockKind::Verse(Verse {
                lines: vec![
                    line("Arms, and the man I sing, who, forc'd by fate,"),
                    line("And haughty Juno's unrelenting hate,"),
                    line("Expell'd and exil'd, left the Trojan shore."),
                ],
                ..Default::default()
            }),
            lang: None,
            reference: None,
            line: 0,
        }];
        // The count is off by one: the nearest "haughty" is found.
        let (s, _) = sentence(&blocks, "v", 12, "haughty").unwrap();
        assert!(s.starts_with("Arms, and the man I sing, who, forc'd by fate, / And haughty"));
        assert!(s.ends_with("left the Trojan shore."));
    }

    #[test]
    fn kept_and_let_go_survive_a_reload() {
        let dir = std::env::temp_dir().join(format!("wordhord-hoard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Hoard::load(&dir);
        let k = Kept {
            id: String::new(),
            time: 0,
            word: "mǫgu".into(),
            head: "mǫgr".into(),
            lang: "non".into(),
            pos: "noun".into(),
            sentence: "Hljóðs bið ek allar helgar kindir, meiri ok minni mǫgu Heimdallar.".into(),
            at: 47,
            book: "Völuspá".into(),
            path: "v.epub".into(),
            chapter: 0,
            block: "b1".into(),
        };
        h.keep(k.clone()).unwrap();
        h.keep(Kept {
            sentence: "Another.".into(),
            block: "b2".into(),
            ..k.clone()
        })
        .unwrap();
        let first = h.kept_in("mǫgu", "v.epub", 0, "b1")[0].id.clone();
        h.let_go(&first).unwrap();
        let again = Hoard::load(&dir);
        assert_eq!(again.words().len(), 1);
        assert_eq!(again.words()[0].kept.len(), 1);
        assert_eq!(again.words()[0].kept[0].sentence, "Another.");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn new_words_are_due_and_reviewed_ones_wait() {
        let dir = std::env::temp_dir().join(format!("wordhord-due-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Hoard::load(&dir);
        for head in ["ash", "elm"] {
            h.keep(Kept {
                id: String::new(),
                time: 0,
                word: head.into(),
                head: head.into(),
                lang: "eng".into(),
                pos: "noun".into(),
                sentence: head.into(),
                at: 0,
                book: "B".into(),
                path: "b.epub".into(),
                chapter: 0,
                block: head.into(),
            })
            .unwrap();
        }
        let due = |h: &Hoard| h.due(now()).into_iter().map(|d| d.0).collect::<Vec<_>>();
        assert_eq!(due(&h), ["ash", "elm"]);
        h.review("ash", "eng", 3).unwrap();
        assert_eq!(due(&h), ["elm"]);
        let again = Hoard::load(&dir);
        assert_eq!(again.reviewed("ash", "eng"), 1);
        assert_eq!(due(&again), ["elm"]);
        // Five days on, a word first graded good is due.
        assert_eq!(again.due(now() + 5 * fsrs::DAY).len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
