//! Marks made in books: underlines, so far. A log, `notes.jsonl` in the app
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Underline(Box<Underline>),
    Remove { id: String, time: u64 },
}

pub struct Marks {
    path: PathBuf,
    /// Underlines not removed, oldest first.
    underlines: Vec<Underline>,
}

impl Marks {
    pub fn load(app: &Path) -> Marks {
        let path = app.join("notes.jsonl");
        let mut underlines: Vec<Underline> = Vec::new();
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            // A line cut short by a crash is skipped, not fatal.
            match serde_json::from_str::<Record>(line) {
                Ok(Record::Underline(u)) => underlines.push(*u),
                Ok(Record::Remove { id, .. }) => underlines.retain(|u| u.id != id),
                Err(_) => {}
            }
        }
        Marks { path, underlines }
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

    pub fn underline(&mut self, mut u: Underline) -> Result<(), String> {
        let time = now();
        u.time = time;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        u.id = format!("{time:x}-{nanos:x}");
        self.append(&Record::Underline(Box::new(u.clone())))?;
        self.underlines.push(u);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.append(&Record::Remove {
            id: id.to_string(),
            time: now(),
        })?;
        self.underlines.retain(|u| u.id != id);
        Ok(())
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
}
