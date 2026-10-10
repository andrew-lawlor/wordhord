//! Marks made in books: underlines, and the pen's ink. A log, `notes.jsonl` in the app
//! folder, one record a line: a passage underlined, or an underline
//! removed. Never edited, only added to, as the hoard is.
//!
//! A passage is anchored by words, not by pages: the paragraph or stanza
//! each end is in, and which word of it, counted as the hoard counts. So an
//! underline stays on its words whatever the face, size or measure. The
//! text is kept too, for reading the log without the book.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hoard::now;

/// A word in a chapter: its paragraph or stanza, and which word of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Anchor {
    pub block: String,
    pub word: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underline {
    pub id: String,
    /// Seconds since 1970.
    pub time: u64,
    pub book: String,
    pub path: String,
    pub chapter: usize,
    pub start: Anchor,
    pub end: Anchor,
    pub text: String,
    /// The part of the book it's in, as its contents name it: "Book I".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// A stroke of the pen. Its points are measured from the top left of the
/// word it's anchored to, in that word's line heights, with the pen's
/// width: so it moves with the word, and grows with the type. A stroke in
/// a margin keeps its height from the word, but its place across is a
/// share of the margin's width, so a change of type can't push it off the
/// page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub id: String,
    pub time: u64,
    pub book: String,
    pub path: String,
    pub chapter: usize,
    pub anchor: Anchor,
    /// "left" or "right", for a stroke in a margin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<String>,
    pub points: Vec<[f32; 3]>,
    /// The stroke as it lay on the page it was written on, in pixels: so
    /// the strokes of one note keep their places together, to be shown
    /// elsewhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drawn: Vec<[f32; 3]>,
    /// The line of text it was written beside, and the part of the book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// A ribbon put in a page: anchored to the page's first word, so it marks
/// the same place whatever the type; with the page's first words and the
/// part of the book, to list it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub time: u64,
    pub book: String,
    pub path: String,
    pub chapter: usize,
    pub anchor: Anchor,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Underline(Box<Underline>),
    Ink(Box<Ink>),
    Bookmark(Box<Bookmark>),
    /// A typed note on an underline: the latest for it stands; an empty one
    /// takes it away.
    Note {
        id: String,
        underline: String,
        text: String,
        time: u64,
    },
    Remove {
        id: String,
        time: u64,
    },
}

pub struct Marks {
    path: PathBuf,
    /// Underlines not removed, oldest first.
    underlines: Vec<Underline>,
    /// Strokes of the pen not removed, oldest first.
    inks: Vec<Ink>,
    /// Ribbons not taken out, oldest first.
    bookmarks: Vec<Bookmark>,
    /// Notes, by the underline they're on.
    notes: std::collections::HashMap<String, String>,
}

impl Marks {
    pub fn load(app: &Path) -> Marks {
        let path = app.join("notes.jsonl");
        let mut underlines: Vec<Underline> = Vec::new();
        let mut inks: Vec<Ink> = Vec::new();
        let mut bookmarks: Vec<Bookmark> = Vec::new();
        let mut notes: std::collections::HashMap<String, String> = Default::default();
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            // A line cut short by a crash is skipped, not fatal.
            match serde_json::from_str::<Record>(line) {
                Ok(Record::Underline(u)) => underlines.push(*u),
                Ok(Record::Ink(i)) => inks.push(*i),
                Ok(Record::Bookmark(b)) => bookmarks.push(*b),
                Ok(Record::Note {
                    underline, text, ..
                }) => {
                    if text.trim().is_empty() {
                        notes.remove(&underline);
                    } else {
                        notes.insert(underline, text);
                    }
                }
                Ok(Record::Remove { id, .. }) => {
                    underlines.retain(|u| u.id != id);
                    inks.retain(|i| i.id != id);
                    bookmarks.retain(|b| b.id != id);
                }
                Err(_) => {}
            }
        }
        Marks {
            path,
            underlines,
            inks,
            bookmarks,
            notes,
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

    /// An underline saved; its id, for a note to go on it.
    pub fn underline(&mut self, mut u: Underline) -> Result<String, String> {
        (u.time, u.id) = stamp();
        self.append(&Record::Underline(Box::new(u.clone())))?;
        let id = u.id.clone();
        self.underlines.push(u);
        Ok(id)
    }

    pub fn ink(&mut self, mut i: Ink) -> Result<(), String> {
        (i.time, i.id) = stamp();
        self.append(&Record::Ink(Box::new(i.clone())))?;
        self.inks.push(i);
        Ok(())
    }

    /// The note on an underline, if it has one.
    pub fn note(&self, underline: &str) -> Option<&str> {
        self.notes.get(underline).map(String::as_str)
    }

    /// A note set on an underline; an empty one takes it away.
    pub fn set_note(&mut self, underline: &str, text: &str) -> Result<(), String> {
        let (time, id) = stamp();
        let text = text.trim().to_string();
        self.append(&Record::Note {
            id,
            underline: underline.to_string(),
            text: text.clone(),
            time,
        })?;
        if text.is_empty() {
            self.notes.remove(underline);
        } else {
            self.notes.insert(underline.to_string(), text);
        }
        Ok(())
    }

    pub fn bookmark(&mut self, mut b: Bookmark) -> Result<(), String> {
        (b.time, b.id) = stamp();
        self.append(&Record::Bookmark(Box::new(b.clone())))?;
        self.bookmarks.push(b);
        Ok(())
    }

    /// An underline, a stroke or a ribbon taken away.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.append(&Record::Remove {
            id: id.to_string(),
            time: now(),
        })?;
        self.underlines.retain(|u| u.id != id);
        self.inks.retain(|i| i.id != id);
        self.bookmarks.retain(|b| b.id != id);
        Ok(())
    }

    /// A book's ribbons, in the order of its chapters, then as put in.
    pub fn bookmarks_in(&self, path: &str) -> Vec<&Bookmark> {
        let mut out: Vec<&Bookmark> = self.bookmarks.iter().filter(|b| b.path == path).collect();
        out.sort_by_key(|b| (b.chapter, b.time));
        out
    }

    /// A book's handwritten notes: its strokes, in the order of its
    /// chapters and as written, gathered into notes where they were written
    /// in one chapter with no more than two minutes between them.
    pub fn ink_notes(&self, path: &str) -> Vec<Vec<&Ink>> {
        let mut inks: Vec<&Ink> = self.inks.iter().filter(|i| i.path == path).collect();
        inks.sort_by_key(|i| (i.chapter, i.time));
        let mut notes: Vec<Vec<&Ink>> = Vec::new();
        for ink in inks {
            match notes.last_mut() {
                Some(note)
                    if note.last().is_some_and(|l| {
                        l.chapter == ink.chapter && ink.time.saturating_sub(l.time) <= 120
                    }) =>
                {
                    note.push(ink)
                }
                _ => notes.push(vec![ink]),
            }
        }
        notes
    }

    /// A stroke, by its id.
    pub fn stroke(&self, id: &str) -> Option<&Ink> {
        self.inks.iter().find(|i| i.id == id)
    }

    /// The handwritten note whose first stroke is `id`.
    pub fn ink_note(&self, id: &str) -> Option<Vec<&Ink>> {
        let path = &self.inks.iter().find(|i| i.id == id)?.path;
        self.ink_notes(path).into_iter().find(|n| n[0].id == id)
    }

    /// The strokes in a chapter of a book.
    pub fn ink_in<'a>(&'a self, path: &'a str, chapter: usize) -> impl Iterator<Item = &'a Ink> {
        self.inks
            .iter()
            .filter(move |i| i.path == path && i.chapter == chapter)
    }

    /// Every underline, for the commonplace book: by book, the book
    /// underlined in last first; in each, in the order of its chapters,
    /// then as they were made.
    pub fn by_book(&self) -> Vec<&Underline> {
        let mut books: Vec<(&str, u64)> = Vec::new();
        for u in &self.underlines {
            match books.iter_mut().find(|b| b.0 == u.path) {
                Some(b) => b.1 = b.1.max(u.time),
                None => books.push((&u.path, u.time)),
            }
        }
        books.sort_by_key(|b| std::cmp::Reverse(b.1));
        let mut out: Vec<&Underline> = Vec::new();
        for (path, _) in books {
            let mut these: Vec<&Underline> =
                self.underlines.iter().filter(|u| u.path == path).collect();
            these.sort_by_key(|u| (u.chapter, u.time));
            out.extend(these);
        }
        out
    }

    /// The books underlined in, each once, as `by_book` orders them: path,
    /// title, and how many passages.
    pub fn books(&self) -> Vec<(String, String, usize)> {
        let mut out: Vec<(String, String, usize)> = Vec::new();
        for u in self.by_book() {
            match out.iter_mut().find(|b| b.0 == u.path) {
                Some(b) => b.2 += 1,
                None => out.push((u.path.clone(), u.book.clone(), 1)),
            }
        }
        out
    }

    pub fn count(&self) -> usize {
        self.underlines.len()
    }

    /// The underlines in a chapter of a book.
    pub fn in_chapter<'a>(
        &'a self,
        path: &'a str,
        chapter: usize,
    ) -> impl Iterator<Item = &'a Underline> {
        self.underlines
            .iter()
            .filter(move |u| u.path == path && u.chapter == chapter)
    }
}

/// The time now, and an id made from it.
fn stamp() -> (u64, String) {
    let time = now();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    (time, format!("{time:x}-{nanos:x}"))
}

/// A day, as a reader says it: "8 October", with the year if it isn't
/// this one's.
pub fn day(time: u64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    // Days since 1970 to a civil date (Howard Hinnant's algorithm).
    let civil = |t: u64| {
        let z = (t / 86_400) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        (y, m as usize, d)
    };
    let (y, m, d) = civil(time);
    let (this_year, _, _) = civil(now());
    if y == this_year {
        format!("{d} {}", MONTHS[m - 1])
    } else {
        format!("{d} {} {y}", MONTHS[m - 1])
    }
}

/// Every word of a chapter, across its pages, numbered: for each page,
/// each word's number in the chapter (the two halves of a hyphenated word
/// share one); and for each number, its anchor.
pub struct Words {
    pub on_page: Vec<Vec<usize>>,
    pub anchors: Vec<Anchor>,
}

impl Words {
    pub fn of(t: &wordhord_typeset::page::Typeset) -> Words {
        let mut on_page = Vec::with_capacity(t.pages.len());
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for p in &t.pages {
            let mut numbers = Vec::with_capacity(p.words.len());
            for w in &p.words {
                if w.continues && !anchors.is_empty() {
                    numbers.push(anchors.len() - 1);
                    continue;
                }
                let n = counts.entry(w.block.as_str()).or_default();
                anchors.push(Anchor {
                    block: w.block.clone(),
                    word: *n,
                });
                *n += 1;
                numbers.push(anchors.len() - 1);
            }
            on_page.push(numbers);
        }
        Words { on_page, anchors }
    }

    /// The words, by number, an underline runs over: none if its words
    /// aren't in the chapter as it is now.
    pub fn span(&self, u: &Underline) -> Option<(usize, usize)> {
        let find = |a: &Anchor| self.anchors.iter().position(|b| b == a);
        let (s, e) = (find(&u.start)?, find(&u.end)?);
        Some((s.min(e), s.max(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underlined_and_removed_survive_a_reload() {
        let dir = std::env::temp_dir().join(format!("wordhord-marks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Marks::load(&dir);
        let u = Underline {
            id: String::new(),
            time: 0,
            book: "Beowulf".into(),
            path: "b.epub".into(),
            chapter: 2,
            start: Anchor {
                block: "b7".into(),
                word: 3,
            },
            end: Anchor {
                block: "b8".into(),
                word: 1,
            },
            text: "wordhord onleac".into(),
            section: None,
        };
        m.underline(u.clone()).unwrap();
        m.underline(Underline {
            chapter: 3,
            ..u.clone()
        })
        .unwrap();
        let first = m.in_chapter("b.epub", 2).next().unwrap().id.clone();
        m.remove(&first).unwrap();
        let again = Marks::load(&dir);
        assert_eq!(again.in_chapter("b.epub", 2).count(), 0);
        assert_eq!(again.in_chapter("b.epub", 3).count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn notes_are_the_latest_and_an_empty_one_takes_it_away() {
        let dir = std::env::temp_dir().join(format!("wordhord-notes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Marks::load(&dir);
        m.set_note("u1", "first thought").unwrap();
        m.set_note("u1", "second thought").unwrap();
        m.set_note("u2", "gone soon").unwrap();
        m.set_note("u2", "  ").unwrap();
        let again = Marks::load(&dir);
        assert_eq!(again.note("u1"), Some("second thought"));
        assert_eq!(again.note("u2"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn days_read_as_a_reader_says_them() {
        // 8 October 2026, and 29 February 2024.
        assert!(day(1_791_480_000).starts_with("8 October"));
        assert_eq!(day(1_709_208_000), "29 February 2024");
    }
}
