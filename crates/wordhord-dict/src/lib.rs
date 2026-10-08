//! Dictionary packs (SPEC.md §7): one SQLite file per language, built on
//! the desktop from Wiktionary's structured extract at kaikki.org, and
//! looked up on the device.
//!
//! A pack holds every entry (headword, part of speech, headline, senses
//! with examples, etymology) and every inflected form of every entry, with
//! the form's grammar. Tap *mǫgu* and the pack answers: accusative plural
//! of *mǫgr*, "son, boy, youth".
//!
//! Wiktionary is CC BY-SA; every pack says so in its `meta` table.

use std::collections::BTreeMap;

pub mod dictfile;
use std::io::BufRead;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// A word as looked up: NFC, lowercase, without the punctuation around it.
pub fn key(word: &str) -> String {
    let word: String = word.nfc().collect();
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'' && c != '’')
        .trim_matches(|c: char| c == '\'' || c == '’' || c == '-')
        .to_lowercase()
}

/// A looser key, for when the exact one finds nothing: without accents or
/// vowel marks; œ, ø and ǫ as o, the one long vowel editions spell three
/// ways; æ as ae. *Grœnum* then meets *grǿnum*, and *Völuspá* meets
/// *Vǫluspá*.
pub fn fold(word: &str) -> String {
    key(word)
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .replace('æ', "ae")
        .replace(['œ', 'ø'], "o")
}

/// One meaning of an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sense {
    /// The definition: "son, boy, youth".
    #[serde(rename = "g")]
    pub gloss: String,
    /// "masculine", "poetic", "archaic".
    #[serde(rename = "t", default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(rename = "x", default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// For a form's own entry: the word it's a form of.
    #[serde(rename = "f", default, skip_serializing_if = "Option::is_none")]
    pub form_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    #[serde(rename = "x")]
    pub text: String,
    #[serde(rename = "en", default, skip_serializing_if = "Option::is_none")]
    pub english: Option<String>,
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub word: String,
    /// "noun", "verb", "adj".
    pub pos: String,
    /// The headline, with gender and principal parts: "mǫgr m (genitive
    /// magar, plural megir)".
    pub head: String,
    pub senses: Vec<Sense>,
    pub etymology: Option<String>,
}

/// One reading of a tapped word: the entry, and what the tapped form is of
/// it, as tag sets (`["accusative", "plural"]`), if it isn't the headword.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub grammar: Vec<Vec<String>>,
    pub entry: Entry,
}

impl Reading {
    /// The grammar as a grammar book gives it: "nominative/accusative
    /// plural", or "dative singular or dative plural".
    pub fn grammar(&self) -> Option<String> {
        describe(&self.grammar)
    }
}

/// Grammar-book order, and which tags say nothing worth saying.
const ORDER: [&[&str]; 9] = [
    &["first-person", "second-person", "third-person"],
    &[
        "present",
        "past",
        "preterite",
        "future",
        "perfect",
        "imperfect",
        "pluperfect",
    ],
    &[
        "indicative",
        "subjunctive",
        "imperative",
        "optative",
        "infinitive",
        "participle",
        "supine",
        "gerund",
    ],
    &["active", "passive", "middle", "mediopassive"],
    &[
        "nominative",
        "accusative",
        "genitive",
        "dative",
        "instrumental",
        "ablative",
        "locative",
        "vocative",
    ],
    &["masculine", "feminine", "neuter"],
    &["singular", "dual", "plural"],
    &["definite"],
    &["strong", "weak", "comparative", "superlative"],
];
const SILENT: [&str; 4] = ["indefinite", "error-unknown-tag", "form-of", "alternative"];

fn describe(sets: &[Vec<String>]) -> Option<String> {
    const CASES: &[&str] = ORDER[4];
    // Without the silent tags.
    let clean: Vec<Vec<&str>> = sets
        .iter()
        .map(|set| {
            set.iter()
                .map(String::as_str)
                .filter(|t| !SILENT.contains(t))
                .collect::<Vec<_>>()
        })
        .filter(|set| !set.is_empty())
        .collect();
    // A reading inside a fuller one says nothing more.
    let full: Vec<&Vec<&str>> = clean
        .iter()
        .filter(|k| {
            !clean
                .iter()
                .any(|o| o.len() > k.len() && k.iter().all(|t| o.contains(t)))
        })
        .collect();
    // Readings that differ only in case are one: "nominative/accusative
    // plural".
    let mut merged: Vec<(Vec<&str>, Vec<&str>)> = Vec::new();
    for set in full {
        let mut rest: Vec<&str> = set.iter().copied().filter(|t| !CASES.contains(t)).collect();
        rest.sort_unstable();
        let cases: Vec<&str> = set.iter().copied().filter(|t| CASES.contains(t)).collect();
        match merged.iter_mut().find(|(r, _)| *r == rest) {
            Some((_, c)) => {
                for x in cases {
                    if !c.contains(&x) {
                        c.push(x);
                    }
                }
            }
            None => merged.push((rest, cases)),
        }
    }
    let mut phrases: Vec<String> = Vec::new();
    for (rest, cases) in merged {
        let tags: Vec<&str> = rest.into_iter().chain(cases).collect();
        let mut words = Vec::new();
        let mut used = Vec::new();
        for slot in ORDER {
            let here: Vec<&str> = slot.iter().copied().filter(|t| tags.contains(t)).collect();
            if !here.is_empty() {
                words.push(here.join("/"));
                used.extend(here);
            }
        }
        for t in &tags {
            if !used.contains(t) {
                words.push(t.replace('-', " "));
            }
        }
        let phrase = words.join(" ");
        if !phrases.contains(&phrase) {
            phrases.push(phrase);
        }
    }
    (!phrases.is_empty()).then(|| phrases.join(" or "))
}

pub struct Dictionary {
    db: Connection,
}

impl Dictionary {
    pub fn open(path: &Path) -> Result<Dictionary, String> {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Dictionary { db })
    }

    /// Every reading of a word as it appears in a text, the likeliest
    /// first: names after common words when the word isn't capitalised,
    /// readings only of the past ("obsolete", "archaic") after living ones,
    /// and symbols (ISO codes, chemical symbols) last of all.
    pub fn look_up(&self, word: &str) -> Vec<Reading> {
        let mut out = self.all_readings(word);
        let lowercase = word
            .chars()
            .find(|c| c.is_alphabetic())
            .is_some_and(|c| c.is_lowercase());
        out.sort_by_key(|r| {
            let name = matches!(r.entry.pos.as_str(), "proper noun" | "name");
            let past = !r.entry.senses.is_empty()
                && r.entry.senses.iter().all(|s| {
                    s.tags.iter().any(|t| {
                        matches!(t.as_str(), "obsolete" | "archaic" | "dated" | "historical")
                    })
                });
            let symbol = r.entry.pos == "symbol";
            symbol as u8 * 4 + (name && lowercase) as u8 * 2 + past as u8
        });
        out
    }

    fn all_readings(&self, word: &str) -> Vec<Reading> {
        let mut readings = self.readings("key", &key(word));
        if readings.is_empty() {
            readings = self.readings("fold", &fold(word));
        }
        // A form's own entry ("accusative plural of mǫgr") leads to the
        // headword's.
        let mut out: Vec<Reading> = Vec::new();
        for r in readings {
            let targets: Vec<(String, String)> = r
                .entry
                .senses
                .iter()
                .filter_map(|s| s.form_of.clone().map(|f| (f, s.tags.join(" "))))
                .collect();
            if !targets.is_empty() && r.entry.senses.iter().all(|s| s.form_of.is_some()) {
                for (target, tags) in targets {
                    for entry in self.entries_named(&target) {
                        let grammar = tags_of(&tags);
                        push(&mut out, Reading { grammar, entry });
                    }
                }
            } else {
                push(&mut out, r);
            }
        }
        out
    }

    fn readings(&self, column: &str, value: &str) -> Vec<Reading> {
        let sql = format!(
            "SELECT form.tags, entry.word, entry.pos, entry.head, entry.senses, entry.etymology
             FROM form JOIN entry ON entry.id = form.entry WHERE form.{column} = ?1
             ORDER BY form.tags = '' DESC, entry.id"
        );
        let Ok(mut q) = self.db.prepare_cached(&sql) else {
            return Vec::new();
        };
        let rows = q.query_map(params![value], |row| {
            let tags: String = row.get(0)?;
            Ok(Reading {
                grammar: tags_of(&tags),
                entry: entry_from(row, 1)?,
            })
        });
        rows.map(|r| r.flatten().collect()).unwrap_or_default()
    }

    fn entries_named(&self, word: &str) -> Vec<Entry> {
        let Ok(mut q) = self.db.prepare_cached(
            "SELECT word, pos, head, senses, etymology FROM entry WHERE word = ?1 ORDER BY id",
        ) else {
            return Vec::new();
        };
        let rows = q.query_map(params![word], |row| entry_from(row, 0));
        rows.map(|r| r.flatten().collect()).unwrap_or_default()
    }

    /// What the pack says about itself: language, source, licence.
    pub fn about(&self, key: &str) -> Option<String> {
        self.db
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .ok()
    }
}

fn push(out: &mut Vec<Reading>, r: Reading) {
    let same = out.iter_mut().find(|o| {
        o.entry.word == r.entry.word && o.entry.pos == r.entry.pos && o.entry.head == r.entry.head
    });
    match same {
        // One entry read two ways ("nominative plural", "accusative plural"):
        // one reading, its grammars together.
        Some(o) => {
            for g in r.grammar {
                if !o.grammar.contains(&g) {
                    o.grammar.push(g);
                }
            }
        }
        None => out.push(r),
    }
}

fn tags_of(tags: &str) -> Vec<Vec<String>> {
    let set: Vec<String> = tags.split_whitespace().map(str::to_string).collect();
    if set.is_empty() {
        Vec::new()
    } else {
        vec![set]
    }
}

fn entry_from(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Entry> {
    let senses: String = row.get(at + 3)?;
    Ok(Entry {
        word: row.get(at)?,
        pos: row.get(at + 1)?,
        head: row.get(at + 2)?,
        senses: serde_json::from_str(&senses).unwrap_or_default(),
        etymology: row.get(at + 4)?,
    })
}

// ---- building ----------------------------------------------------------------

/// A kaikki.org record: the parts a pack keeps.
#[derive(Deserialize)]
struct Record {
    word: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    etymology_text: Option<String>,
    #[serde(default)]
    head_templates: Vec<HeadTemplate>,
    #[serde(default)]
    senses: Vec<RawSense>,
    #[serde(default)]
    forms: Vec<RawForm>,
}

#[derive(Deserialize)]
struct HeadTemplate {
    #[serde(default)]
    expansion: String,
}

#[derive(Deserialize)]
struct RawSense {
    #[serde(default)]
    glosses: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    examples: Vec<RawExample>,
    #[serde(default)]
    form_of: Vec<RawFormOf>,
}

#[derive(Deserialize)]
struct RawExample {
    #[serde(default)]
    text: String,
    #[serde(default)]
    english: Option<String>,
    #[serde(default)]
    translation: Option<String>,
    #[serde(rename = "ref", default)]
    reference: Option<String>,
}

#[derive(Deserialize)]
struct RawFormOf {
    word: String,
}

#[derive(Deserialize)]
struct RawForm {
    form: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Tags that describe an inflection table, not a form in it.
const TABLE_TAGS: [&str; 5] = [
    "table-tags",
    "inflection-template",
    "class",
    "romanization",
    "canonical",
];
/// Tags that only clutter a reading.
const QUIET_TAGS: [&str; 3] = ["form-of", "error-unknown-tag", "alt-of"];

/// Writes a pack: entries and their forms, then indexes and what the pack
/// says about itself.
pub(crate) struct Builder {
    db: Connection,
    entries: usize,
    forms: usize,
}

impl Builder {
    pub(crate) fn new(out: &Path) -> Result<Builder, String> {
        let _ = std::fs::remove_file(out);
        let db = Connection::open(out).map_err(|e| format!("{}: {e}", out.display()))?;
        db.execute_batch(
            "PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE entry (id INTEGER PRIMARY KEY, word TEXT NOT NULL, pos TEXT NOT NULL,
                                 head TEXT NOT NULL, senses TEXT NOT NULL, etymology TEXT);
             CREATE TABLE form (key TEXT NOT NULL, fold TEXT NOT NULL, entry INTEGER NOT NULL, tags TEXT NOT NULL);
             BEGIN;",
        )
        .map_err(|e| e.to_string())?;
        Ok(Builder {
            db,
            entries: 0,
            forms: 0,
        })
    }

    /// An entry, and its forms with their grammar ("" if unknown). The
    /// headword is a form of itself.
    pub(crate) fn add(
        &mut self,
        word: &str,
        pos: &str,
        head: &str,
        senses: &[Sense],
        etymology: Option<&str>,
        forms: &[(String, String)],
    ) -> Result<(), String> {
        self.entries += 1;
        let id = self.entries as i64;
        let json = serde_json::to_string(senses).map_err(|e| e.to_string())?;
        self.db
            .prepare_cached("INSERT INTO entry (id, word, pos, head, senses, etymology) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .and_then(|mut q| q.execute(params![id, word, pos, head, json, etymology]))
            .map_err(|e| e.to_string())?;
        let mut seen: BTreeMap<(String, String), ()> = BTreeMap::new();
        seen.insert((key(word), String::new()), ());
        for (form, tags) in forms {
            seen.insert((key(form), tags.clone()), ());
        }
        for (k, tags) in seen.into_keys() {
            if k.is_empty() {
                continue;
            }
            self.db
                .prepare_cached("INSERT INTO form (key, fold, entry, tags) VALUES (?1, ?2, ?3, ?4)")
                .and_then(|mut q| q.execute(params![k, fold(&k), id, tags]))
                .map_err(|e| e.to_string())?;
            self.forms += 1;
        }
        Ok(())
    }

    pub(crate) fn finish(self, language: &str, source: &Path) -> Result<(usize, usize), String> {
        let source = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.db
            .execute_batch(
                "COMMIT;
                 CREATE INDEX form_key ON form (key);
                 CREATE INDEX form_fold ON form (fold);
                 CREATE INDEX entry_word ON entry (word);",
            )
            .map_err(|e| e.to_string())?;
        for (k, v) in [
            ("language", language),
            ("source", "Wiktionary"),
            ("file", source.as_str()),
            ("licence", "CC BY-SA 4.0 and GFDL, as Wiktionary"),
        ] {
            self.db
                .execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    params![k, v],
                )
                .map_err(|e| e.to_string())?;
        }
        self.db
            .execute_batch("VACUUM;")
            .map_err(|e| e.to_string())?;
        Ok((self.entries, self.forms))
    }
}

/// Builds a pack from a kaikki.org JSON Lines file. Returns how many entries
/// and forms went in.
pub fn build(jsonl: &Path, language: &str, out: &Path) -> Result<(usize, usize), String> {
    let file = std::fs::File::open(jsonl).map_err(|e| format!("{}: {e}", jsonl.display()))?;
    let mut builder = Builder::new(out)?;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(r) = serde_json::from_str::<Record>(&line) else {
            continue;
        };
        let senses: Vec<Sense> = r
            .senses
            .iter()
            .filter(|s| !s.glosses.is_empty())
            .map(|s| Sense {
                gloss: s.glosses.last().cloned().unwrap_or_default(),
                tags: s
                    .tags
                    .iter()
                    .filter(|t| !QUIET_TAGS.contains(&t.as_str()))
                    .cloned()
                    .collect(),
                examples: s
                    .examples
                    .iter()
                    .filter(|e| !e.text.is_empty())
                    .take(2)
                    .map(|e| Example {
                        text: e.text.clone(),
                        english: e.english.clone().or_else(|| e.translation.clone()),
                        source: e.reference.clone(),
                    })
                    .collect(),
                form_of: s.form_of.first().map(|f| f.word.clone()),
            })
            .collect();
        if senses.is_empty() {
            continue;
        }
        let head = r
            .head_templates
            .first()
            .map(|h| h.expansion.clone())
            .unwrap_or_else(|| r.word.clone());
        let forms: Vec<(String, String)> = r
            .forms
            .iter()
            .filter(|f| {
                !f.tags.iter().any(|t| TABLE_TAGS.contains(&t.as_str()))
                    && !f.form.trim().is_empty()
                    && f.form != "-"
            })
            .map(|f| {
                let tags: Vec<&str> = f
                    .tags
                    .iter()
                    .map(String::as_str)
                    .filter(|t| !QUIET_TAGS.contains(t))
                    .collect();
                (f.form.clone(), tags.join(" "))
            })
            .collect();
        builder.add(
            &r.word,
            &r.pos,
            &head,
            &senses,
            r.etymology_text.as_deref(),
            &forms,
        )?;
    }
    builder.finish(language, jsonl)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDS: &str = r#"{"word":"mǫgr","pos":"noun","etymology_text":"From Proto-Germanic *maguz.","head_templates":[{"expansion":"mǫgr m"}],"senses":[{"glosses":["son, boy, youth"],"tags":["masculine"],"examples":[{"text":"mǫgu Heimdallar","english":"sons of Heimdall","ref":"Vǫluspá 1"}]}],"forms":[{"form":"non-decl-m-u","tags":["inflection-template"]},{"form":"mǫgu","tags":["accusative","indefinite","plural"]},{"form":"megir","tags":["indefinite","nominative","plural"]}]}
{"word":"kind","pos":"noun","head_templates":[{"expansion":"kind f"}],"senses":[{"glosses":["kin, kindred"]}],"forms":[{"form":"kindir","tags":["nominative","plural"]},{"form":"kindir","tags":["accusative","plural"]}]}
{"word":"grœnn","pos":"adj","senses":[{"glosses":["green"]}],"forms":[{"form":"grœnum","tags":["dative","plural"]}]}
{"word":"megir","pos":"noun","senses":[{"glosses":["nominative plural of mǫgr"],"tags":["form-of","nominative","plural"],"form_of":[{"word":"mǫgr"}]}]}"#;

    fn pack() -> (Dictionary, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("wordhord-dict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("non.jsonl");
        std::fs::write(&src, RECORDS).unwrap();
        let out = dir.join(format!("non-{:?}.sqlite", std::thread::current().id()));
        let (entries, _) = build(&src, "non", &out).unwrap();
        assert_eq!(entries, 4);
        (Dictionary::open(&out).unwrap(), out)
    }

    #[test]
    fn an_inflected_form_finds_its_headword_and_grammar() {
        let (d, _) = pack();
        let r = d.look_up("mǫgu,");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entry.word, "mǫgr");
        assert_eq!(r[0].grammar().as_deref(), Some("accusative plural"));
        assert_eq!(r[0].entry.senses[0].gloss, "son, boy, youth");
        assert_eq!(
            r[0].entry.senses[0].examples[0].english.as_deref(),
            Some("sons of Heimdall")
        );
        assert_eq!(
            d.about("licence").as_deref(),
            Some("CC BY-SA 4.0 and GFDL, as Wiktionary")
        );
    }

    #[test]
    fn two_grammars_of_one_entry_are_one_reading() {
        let (d, _) = pack();
        let r = d.look_up("Kindir");
        assert_eq!(r.len(), 1);
        assert_eq!(
            r[0].grammar().as_deref(),
            Some("nominative/accusative plural")
        );
    }

    #[test]
    fn a_forms_own_entry_leads_to_the_headword() {
        let (d, _) = pack();
        // "megir" is a form in mǫgr's table and an entry of its own that
        // points at mǫgr: one reading either way.
        let r = d.look_up("megir");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entry.word, "mǫgr");
    }

    #[test]
    fn spelling_variants_fall_back_to_folding() {
        let (d, _) = pack();
        // An edition's ǿ for the dictionary's œ.
        let r = d.look_up("grǿnum");
        assert_eq!(r.first().map(|r| r.entry.word.as_str()), Some("grœnn"));
        assert!(d.look_up("nothing").is_empty());
    }

    #[test]
    fn grammar_reads_as_a_grammar_book_gives_it() {
        let sets = |s: &[&str]| {
            s.iter()
                .map(|t| t.split(' ').map(String::from).collect())
                .collect::<Vec<_>>()
        };
        // Kaikki's alphabetical tags, a contained reading, the silent
        // "indefinite".
        let g = describe(&sets(&[
            "accusative nominative plural",
            "accusative indefinite plural",
            "indefinite nominative plural",
            "plural",
        ]));
        assert_eq!(g.as_deref(), Some("nominative/accusative plural"));
        let g = describe(&sets(&[
            "dative masculine singular strong",
            "dative plural strong",
        ]));
        assert_eq!(
            g.as_deref(),
            Some("dative masculine singular strong or dative plural strong")
        );
        assert_eq!(describe(&[]), None);
    }

    #[test]
    fn living_words_before_dead_ones_and_names() {
        let dir = std::env::temp_dir().join(format!("wordhord-dict-rank-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("en.jsonl");
        std::fs::write(
            &src,
            r#"{"word":"went","pos":"noun","senses":[{"glosses":["a path"],"tags":["obsolete"]}]}
{"word":"Went","pos":"proper noun","senses":[{"glosses":["a river"]}]}
{"word":"go","pos":"verb","senses":[{"glosses":["to move"]}],"forms":[{"form":"went","tags":["past"]}]}"#,
        )
        .unwrap();
        let out = dir.join("en.sqlite");
        build(&src, "eng", &out).unwrap();
        let d = Dictionary::open(&out).unwrap();
        let order: Vec<String> = d
            .look_up("went")
            .iter()
            .map(|r| format!("{} {}", r.entry.word, r.entry.pos))
            .collect();
        assert_eq!(order, ["go verb", "went noun", "Went proper noun"]);
        // Capitalised in the text, a name is as likely as anything.
        assert_eq!(d.look_up("Went")[0].entry.word, "Went");
    }

    #[test]
    fn keys_and_folds() {
        assert_eq!(key("“Mǫgu,”"), "mǫgu");
        assert_eq!(fold("Vǫluspá"), "voluspa");
        assert_eq!(fold("grœnum"), "gronum");
        assert_eq!(fold("grǿnum"), "gronum");
    }
}
