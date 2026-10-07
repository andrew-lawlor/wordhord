//! The dictionary on the device: the packs in the app folder's
//! `dictionaries/`, each named by its language (`non.sqlite`), and a looked-up
//! word set as a printed dictionary sets it:
//!
//! ```text
//! mǫgr  NOUN  m (genitive magar, plural megir)
//! mǫgu: accusative plural
//! son, boy, youth
//!     meiri ok minni / mǫgu Heimdallar — greater and smaller / sons of Heimdall
//! ETYMOLOGY. From Proto-Norse …
//! ```

use std::path::Path;

use wordhord_book::model::{Block, BlockKind, Inline, SpanKind};
use wordhord_dict::{Dictionary, Reading};

/// The dictionaries installed, by language code.
pub struct Lexicon {
    packs: Vec<(String, Dictionary)>,
}

impl Lexicon {
    pub fn open(dir: &Path) -> Lexicon {
        let mut packs: Vec<(String, Dictionary)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if path.extension()? != "sqlite" {
                    return None;
                }
                let lang = path.file_stem()?.to_string_lossy().into_owned();
                Some((lang, Dictionary::open(&path).ok()?))
            })
            .collect();
        packs.sort_by(|a, b| a.0.cmp(&b.0));
        eprintln!(
            "dictionaries: {}",
            packs
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        Lexicon { packs }
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    /// A word's readings: from the pack for the book's language first, then
    /// any other that knows it. The language found in, and the readings.
    pub fn look_up(&self, word: &str, lang: &str) -> Option<(&str, Vec<Reading>)> {
        let wanted = three_letter(lang);
        let first = self.packs.iter().filter(|(l, _)| *l == wanted);
        let rest = self.packs.iter().filter(|(l, _)| *l != wanted);
        first.chain(rest).find_map(|(l, d)| {
            let r = d.look_up(word);
            (!r.is_empty()).then_some((l.as_str(), r))
        })
    }
}

/// ISO 639-1 codes, as EPUBs mostly give them, as the 639-3 codes packs are
/// named by. Codes already three letters long pass through.
pub fn three_letter(lang: &str) -> String {
    let lang = lang.split(['-', '_']).next().unwrap_or(lang).to_lowercase();
    let code = match lang.as_str() {
        "en" => "eng",
        "de" => "deu",
        "fr" => "fra",
        "es" => "spa",
        "it" => "ita",
        "la" => "lat",
        "is" => "isl",
        "no" | "nb" | "nn" => "nor",
        "da" => "dan",
        "sv" => "swe",
        "nl" => "nld",
        "pt" => "por",
        "ru" => "rus",
        "el" => "ell",
        "ga" => "gle",
        "cy" => "cym",
        "fi" => "fin",
        other => other,
    };
    code.to_string()
}

/// A language's name, for saying what was looked in.
pub fn language_name(code: &str) -> &str {
    match code {
        "eng" => "English",
        "non" => "Old Norse",
        "ang" => "Old English",
        "lat" => "Latin",
        "grc" => "Ancient Greek",
        "isl" => "Icelandic",
        "enm" => "Middle English",
        "goh" => "Old High German",
        "got" => "Gothic",
        "sga" => "Old Irish",
        other => other,
    }
}

fn text(t: impl Into<String>) -> Inline {
    Inline::Text(t.into())
}

fn small_caps(t: impl Into<String>) -> Inline {
    Inline::Span {
        kind: SpanKind::SmallCaps,
        lang: None,
        inlines: vec![text(t)],
    }
}

fn para(inlines: Vec<Inline>) -> Block {
    Block {
        id: None,
        kind: BlockKind::Paragraph(inlines),
        lang: None,
        reference: None,
        line: 0,
    }
}

/// Tags the headline already says, or that say nothing to a reader.
const SAID: [&str; 5] = ["masculine", "feminine", "neuter", "form-of", "alternative"];

/// A word's readings as blocks, set as a dictionary sets an entry. At most
/// four readings, and three senses each: enough for a panel.
pub fn entry_blocks(word: &str, readings: &[Reading]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let shown: Vec<&Reading> = readings.iter().take(4).collect();
    for (k, r) in shown.iter().enumerate() {
        let e = &r.entry;
        // A word's parts of speech follow on; a different word is set apart.
        if k > 0 && shown[k - 1].entry.word != e.word {
            blocks.push(Block {
                id: None,
                kind: BlockKind::SceneBreak,
                lang: None,
                reference: None,
                line: 0,
            });
        }
        // **mǫgr** NOUN *m (genitive magar, plural megir)*
        let mut head = vec![
            Inline::Strong(vec![text(&e.word)]),
            text("  "),
            small_caps(&e.pos),
        ];
        let rest = e
            .head
            .strip_prefix(e.word.as_str())
            .unwrap_or(&e.head)
            .trim();
        if !rest.is_empty() {
            head.push(text("  "));
            head.push(Inline::Emphasis(vec![text(rest)]));
        }
        blocks.push(para(head));
        // *mǫgu*: accusative plural
        if let Some(g) = r.grammar() {
            blocks.push(para(vec![
                Inline::Emphasis(vec![text(word)]),
                text(format!(": {g}")),
            ]));
        }
        let senses: Vec<_> = e
            .senses
            .iter()
            .filter(|s| !s.gloss.is_empty())
            .take(3)
            .collect();
        let mut items = Vec::new();
        for s in &senses {
            let mut line = vec![text(&s.gloss)];
            let tags: Vec<&str> = s
                .tags
                .iter()
                .map(String::as_str)
                .filter(|t| !SAID.contains(t))
                .collect();
            if !tags.is_empty() {
                line.push(text(" "));
                line.push(Inline::Emphasis(vec![text(format!(
                    "({})",
                    tags.join(", ")
                ))]));
            }
            let mut item = vec![para(line)];
            // The example set off as a quotation: the text in italic, its
            // English after.
            if let Some(x) = s.examples.first() {
                let mut ex = vec![Inline::Emphasis(vec![text(&x.text)])];
                if let Some(en) = &x.english {
                    ex.push(text(format!(" — {en}")));
                }
                item.push(Block {
                    id: None,
                    kind: BlockKind::Quote(vec![para(ex)]),
                    lang: None,
                    reference: None,
                    line: 0,
                });
            }
            items.push(item);
        }
        if items.len() > 1 {
            blocks.push(Block {
                id: None,
                kind: BlockKind::List {
                    ordered: true,
                    start: 1,
                    items,
                },
                lang: None,
                reference: None,
                line: 0,
            });
        } else {
            blocks.extend(items.into_iter().flatten());
        }
        // The etymology once, after all of a word's parts of speech, as a
        // printed dictionary sets it.
        let word_goes_on = shown.get(k + 1).is_some_and(|n| n.entry.word == e.word);
        let etymology = shown[..=k]
            .iter()
            .rev()
            .take_while(|p| p.entry.word == e.word)
            .find_map(|p| p.entry.etymology.as_deref().filter(|t| !t.is_empty()));
        if let Some(ety) = etymology.filter(|_| !word_goes_on) {
            let ety = if ety.chars().count() > 320 {
                let cut: String = ety.chars().take(320).collect();
                format!("{}…", cut.trim_end())
            } else {
                ety.to_string()
            };
            blocks.push(para(vec![
                small_caps("etymology"),
                text(format!(". {ety}")),
            ]));
        }
    }
    blocks.push(para(vec![Inline::Emphasis(vec![text(
        "From Wiktionary, CC BY-SA.",
    )])]));
    blocks
}

/// What to say when nothing was found.
pub fn not_found(word: &str, lang: &str, any: bool) -> Vec<Block> {
    let said = if any {
        format!("Nothing for “{word}” in the dictionaries here.")
    } else {
        format!(
            "No dictionary for {} is installed. Dictionaries go in Wordhord’s dictionaries folder.",
            language_name(&three_letter(lang))
        )
    };
    vec![para(vec![Inline::Emphasis(vec![text(said)])])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_by_their_three_letter_codes() {
        assert_eq!(three_letter("en-GB"), "eng");
        assert_eq!(three_letter("non"), "non");
        assert_eq!(three_letter("LA"), "lat");
    }
}
