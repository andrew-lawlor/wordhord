//! Searching a book: every place its words follow the words asked for,
//! chapter by chapter. Words are compared loosely, as the dictionary's
//! second try is: no case, no accents, œ, ø and ǫ alike; and the last word
//! asked for may be the start of a longer one, so "rumou" finds "rumour".

use wordhord_book::model::{self, Block};

use crate::book::Book;
use crate::marks::Anchor;

/// Whether every word asked for starts a word of `haystack`, loosely:
/// for the library's titles and authors, and the commonplace book.
pub fn matches(haystack: &str, query: &str) -> bool {
    let words: Vec<String> = haystack
        .split(|c: char| c.is_whitespace() || c == '-' || c == '/')
        .map(loose)
        .collect();
    query
        .split_whitespace()
        .map(loose)
        .filter(|q| !q.is_empty())
        .all(|q| words.iter().any(|w| w.starts_with(q.as_str())))
}

/// A place found: its chapter, its first word, and the words around it.
pub struct Found {
    pub chapter: usize,
    pub anchor: Anchor,
    pub before: String,
    pub words: String,
    pub after: String,
}

/// How many words of context either side.
const AROUND: usize = 5;

pub fn loose(word: &str) -> String {
    wordhord_dict::fold(&word.replace('’', "'"))
}

/// A chapter's text for searching: each block the pages can find again,
/// with its words, and the same words as they're compared, both joined by
/// single spaces, so the nth of each go together.
pub type Chapter = Vec<(String, String, String)>;

/// A book's text for searching, read a chapter at a time the first time a
/// search needs it, and kept while the book is open: so a second search,
/// or the same one refined, is quick.
#[derive(Default)]
pub struct Text {
    chapters: std::cell::RefCell<Vec<Option<std::rc::Rc<Chapter>>>>,
}

impl Text {
    fn chapter(&self, book: &Book, k: usize) -> std::rc::Rc<Chapter> {
        let mut all = self.chapters.borrow_mut();
        if all.len() < book.chapters() {
            all.resize(book.chapters(), None);
        }
        all[k]
            .get_or_insert_with(|| std::rc::Rc::new(read(&book.blocks(k))))
            .clone()
    }
}

fn read(blocks: &[Block]) -> Chapter {
    let mut all = Vec::new();
    model::walk(blocks, &mut all);
    all.into_iter()
        .filter_map(|b| {
            let id = b.id.clone()?;
            let words: Vec<String> = model::block_inlines(b)
                .into_iter()
                .flat_map(|line| {
                    model::plain_text(line)
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect();
            let keys: Vec<String> = words.iter().map(|w| loose(w)).collect();
            Some((id, words.join(" "), keys.join(" ")))
        })
        .collect()
}

/// The places in `book` that match `query`, at most `limit`, in reading
/// order.
pub fn search(book: &Book, text: &Text, query: &str, limit: usize) -> Vec<Found> {
    let asked: Vec<String> = query
        .split_whitespace()
        .map(loose)
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    if asked.is_empty() {
        return out;
    }
    for chapter in 0..book.chapters() {
        in_chapter(
            &text.chapter(book, chapter),
            chapter,
            &asked,
            limit,
            &mut out,
        );
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn in_chapter(
    text: &Chapter,
    chapter: usize,
    asked: &[String],
    limit: usize,
    out: &mut Vec<Found>,
) {
    for (id, words, keys) in text {
        let words: Vec<&str> = words.split(' ').collect();
        let keys: Vec<&str> = keys.split(' ').collect();
        let n = asked.len();
        if keys.len() < n {
            continue;
        }
        for at in 0..=keys.len() - n {
            let fits = asked.iter().enumerate().all(|(j, a)| {
                let k = keys[at + j];
                if j + 1 == n {
                    k.starts_with(a.as_str())
                } else {
                    k == a
                }
            });
            if !fits {
                continue;
            }
            let from = at.saturating_sub(AROUND);
            let to = (at + n + AROUND).min(words.len());
            let mut before = words[from..at].join(" ");
            if from > 0 {
                before.insert_str(0, "… ");
            }
            let mut after = words[at + n..to].join(" ");
            if to < words.len() {
                after.push_str(" …");
            }
            out.push(Found {
                chapter,
                anchor: Anchor {
                    block: id.clone(),
                    word: at,
                },
                before,
                words: words[at..at + n].join(" "),
                after,
            });
            if out.len() >= limit {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordhord_book::model::{BlockKind, Inline};

    #[test]
    fn every_word_asked_for_starts_one_there() {
        let book = "The Brothers Karamazov Dostoevsky, Fyodor Fiction / Classics";
        assert!(matches(book, "dost kar"));
        assert!(matches(book, "Karamázov"));
        assert!(!matches(book, "dost idiot"));
        assert!(matches("Völuspá", "voluspa"));
    }

    #[test]
    fn words_are_found_loosely_with_the_last_as_a_start() {
        let blocks = [Block {
            id: Some("b1".into()),
            kind: BlockKind::Paragraph(vec![Inline::Text(
                "Yet she had heard an ancient Rumour fly, of Vǫluspá's fame.".into(),
            )]),
            lang: None,
            reference: None,
            line: 0,
        }];
        let text = read(&blocks);
        let mut out = Vec::new();
        in_chapter(&text, 3, &["ancient".into(), "rumou".into()], 10, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].anchor.word, 5);
        assert_eq!(out[0].words, "ancient Rumour");
        assert_eq!(out[0].before, "Yet she had heard an");
        out.clear();
        in_chapter(&text, 3, &[loose("voluspa")], 10, &mut out);
        assert_eq!(out[0].words, "Vǫluspá's");
    }
}
