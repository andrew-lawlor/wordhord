//! Searching a book: every place its words follow the words asked for,
//! chapter by chapter. Words are compared loosely, as the dictionary's
//! second try is: no case, no accents, œ, ø and ǫ alike; and the last word
//! asked for may be the start of a longer one, so "rumou" finds "rumour".

use wordhord_book::model::{self, Block};

use crate::book::Book;
use crate::marks::Anchor;

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

/// The places in `book` that match `query`, at most `limit`, in reading
/// order.
pub fn search(book: &Book, query: &str, limit: usize) -> Vec<Found> {
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
        in_blocks(&book.blocks(chapter), chapter, &asked, limit, &mut out);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn in_blocks(
    blocks: &[Block],
    chapter: usize,
    asked: &[String],
    limit: usize,
    out: &mut Vec<Found>,
) {
    let mut all = Vec::new();
    model::walk(blocks, &mut all);
    for b in all {
        // Only blocks the pages can find again.
        let Some(id) = b.id.as_deref() else { continue };
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
        let n = asked.len();
        for at in 0..keys.len().saturating_sub(n - 1) {
            let fits = asked.iter().enumerate().all(|(j, a)| {
                let k = &keys[at + j];
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
                    block: id.to_string(),
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
        let mut out = Vec::new();
        in_blocks(
            &blocks,
            3,
            &["ancient".into(), "rumou".into()],
            10,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].anchor.word, 5);
        assert_eq!(out[0].words, "ancient Rumour");
        assert_eq!(out[0].before, "Yet she had heard an");
        out.clear();
        in_blocks(&blocks, 3, &[loose("voluspa")], 10, &mut out);
        assert_eq!(out[0].words, "Vǫluspá's");
    }
}
