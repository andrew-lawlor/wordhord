//! A Wordhord book's source: a folder with `book.toml`, an optional
//! `names.toml`, and chapters in `text/*.dj` (SPEC.md §4, VOCABULARY.md).
//! Loading it parses, numbers and checks it.

pub mod align;
pub mod model;
mod parse;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// Something wrong with a book, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let what = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if self.line > 0 {
            write!(f, "{}:{}: {what}: {}", self.file, self.line, self.message)
        } else {
            write!(f, "{}: {what}: {}", self.file, self.message)
        }
    }
}

/// `book.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub lang: String,
    pub author: Option<String>,
    pub translator: Option<String>,
    pub year: Option<i64>,
    pub license: Option<String>,
    pub source: Option<String>,
    /// The book this one translates.
    pub original: Option<String>,
    pub refs: Option<RefScheme>,
    pub edition: Option<Edition>,
}

/// How a work is cited: `[refs]` in `book.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefScheme {
    /// The work's abbreviation, for a book of one work: `bw`.
    pub prefix: Option<String>,
    /// Several works in one book, by abbreviation: `vsp = "Völuspá"`.
    pub works: Option<BTreeMap<String, String>>,
    /// `line`, `stanza` or `stanza.line`, for verse.
    pub unit: Option<String>,
    /// For prose: the levels after the prefix, e.g. `["book", "section"]`.
    pub levels: Option<Vec<String>>,
}

impl RefScheme {
    /// Every work's abbreviation.
    pub fn prefixes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.prefix.iter().map(String::as_str).collect();
        if let Some(w) = &self.works {
            out.extend(w.keys().map(String::as_str));
        }
        out
    }

    /// Whether a reference starts with one of the book's works.
    pub fn owns(&self, reference: &str) -> bool {
        let first = reference.split('.').next().unwrap_or_default();
        self.prefixes().contains(&first)
    }

    fn named(&self) -> String {
        self.prefixes()
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The printed edition a book came from.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub key: String,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<i64>,
}

/// An entry in `names.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Name {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub also: Vec<String>,
    pub note: Option<String>,
    pub wikidata: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Chapter {
    /// The file, relative to the book: `text/01.dj`.
    pub file: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub dir: PathBuf,
    pub meta: Meta,
    pub names: BTreeMap<String, Name>,
    pub chapters: Vec<Chapter>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Book {
    /// Loads, numbers and checks a book. Fails only when `book.toml` can't
    /// be read; everything else is a diagnostic.
    pub fn load(dir: &Path) -> Result<Book, Diagnostic> {
        let fatal = |file: &str, message: String| Diagnostic {
            severity: Severity::Error,
            file: file.into(),
            line: 0,
            message,
        };
        let text = std::fs::read_to_string(dir.join("book.toml"))
            .map_err(|e| fatal("book.toml", e.to_string()))?;
        let meta: Meta =
            toml::from_str(&text).map_err(|e| fatal("book.toml", e.message().to_string()))?;
        let mut diagnostics = Vec::new();

        let names = match std::fs::read_to_string(dir.join("names.toml")) {
            Ok(text) => match toml::from_str::<BTreeMap<String, Name>>(&text) {
                Ok(n) => n,
                Err(e) => {
                    diagnostics.push(fatal("names.toml", e.message().to_string()));
                    BTreeMap::new()
                }
            },
            Err(_) => BTreeMap::new(),
        };

        let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("text"))
            .map(|d| {
                d.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "dj"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        if files.is_empty() {
            diagnostics.push(fatal("text", "no chapters: put them in text/*.dj".into()));
        }
        let mut chapters = Vec::new();
        for path in files {
            let file = format!(
                "text/{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    let (blocks, d) = parse::parse(&src, &file);
                    diagnostics.extend(d);
                    chapters.push(Chapter { file, blocks });
                }
                Err(e) => diagnostics.push(fatal(&file, e.to_string())),
            }
        }

        let mut book = Book {
            dir: dir.to_path_buf(),
            meta,
            names,
            chapters,
            diagnostics,
        };
        book.number();
        book.check();
        book.diagnostics
            .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
        Ok(book)
    }

    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// Every block in the book, with its chapter file, in reading order.
    pub fn blocks(&self) -> Vec<(&str, &Block)> {
        let mut out = Vec::new();
        for c in &self.chapters {
            let mut all = Vec::new();
            walk(&c.blocks, &mut all);
            out.extend(all.into_iter().map(|b| (c.file.as_str(), b)));
        }
        out
    }

    fn report(&mut self, severity: Severity, file: &str, line: usize, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            severity,
            file: file.into(),
            line,
            message: message.into(),
        });
    }

    /// Gives verse lines their canonical references (VOCABULARY.md,
    /// References): counted from `start=` for line schemes, within the
    /// stanza for `stanza.line`.
    fn number(&mut self) {
        let scheme = self.meta.refs.clone();
        let mut found = Vec::new();
        for c in &mut self.chapters {
            number_blocks(&mut c.blocks, scheme.as_ref(), &c.file, &mut found);
        }
        for (file, line, message) in found {
            self.report(Severity::Error, &file, line, message);
        }
    }

    fn check(&mut self) {
        let mut found: Vec<(Severity, String, usize, String)> = Vec::new();
        let mut push = |s, f: &str, l, m: String| found.push((s, f.to_string(), l, m));

        if self.meta.lang.len() != 3 || !self.meta.lang.bytes().all(|b| b.is_ascii_lowercase()) {
            push(
                Severity::Error,
                "book.toml",
                0,
                format!("`lang = \"{}\"` isn't an ISO 639-3 code", self.meta.lang),
            );
        }

        // IDs: every top-level block has one, and no two blocks share one.
        let mut ids: HashMap<String, (String, usize)> = HashMap::new();
        for c in &self.chapters {
            for b in &c.blocks {
                if b.id.is_none() && b.kind != BlockKind::SceneBreak {
                    push(
                        Severity::Error,
                        &c.file,
                        b.line,
                        "this block has no #id: every block needs a stable one".into(),
                    );
                }
            }
        }
        for (file, b) in self.blocks() {
            let Some(id) = &b.id else { continue };
            if let Some((f, l)) = ids.get(id) {
                push(
                    Severity::Error,
                    file,
                    b.line,
                    format!("#{id} is already used at {f}:{l}"),
                );
            } else {
                ids.insert(id.clone(), (file.to_string(), b.line));
            }
            if !id_looks_right(id) {
                push(
                    Severity::Warning,
                    file,
                    b.line,
                    format!("#{id} isn't in the form `ch01-0042`"),
                );
            }
        }

        // Names, links, references and source pages, inline by inline.
        let mut used = HashSet::new();
        let prefix = self.meta.refs.as_ref().map(|r| r.prefixes().len());
        if let Some(r) = &self.meta.refs {
            if r.prefix.is_some() == r.works.is_some() {
                push(
                    Severity::Error,
                    "book.toml",
                    0,
                    "[refs] needs either `prefix` (one work) or `works` (several), not both or neither".into(),
                );
            }
        }
        for (file, b) in self.blocks() {
            if let (Some(r), BlockKind::Paragraph(_)) = (&b.reference, &b.kind) {
                check_prose_ref(r, self.meta.refs.as_ref(), file, b.line, &mut push);
            }
            if let BlockKind::Verse(v) | BlockKind::Stanza(v) = &b.kind {
                if b.reference.is_some() && prefix.is_none() {
                    push(
                        Severity::Error,
                        file,
                        b.line,
                        "`ref=` needs a [refs] scheme in book.toml".into(),
                    );
                }
                if let Some(o) = &v.original {
                    match &self.meta.original {
                        Some(m) if m != o => push(
                            Severity::Error,
                            file,
                            b.line,
                            format!("`original={o}`, but book.toml says `original = \"{m}\"`"),
                        ),
                        None => push(
                            Severity::Warning,
                            file,
                            b.line,
                            format!("`original={o}`, but book.toml names no original"),
                        ),
                        _ => {}
                    }
                }
            }
            for inlines in block_inlines(b) {
                let mut all = Vec::new();
                walk_inlines(inlines, &mut all);
                for i in all {
                    match i {
                        Inline::Span {
                            kind:
                                SpanKind::Name {
                                    kind,
                                    key: Some(key),
                                    ..
                                },
                            ..
                        } => {
                            used.insert(key.clone());
                            match self.names.get(key) {
                                None => push(
                                    Severity::Error,
                                    file,
                                    b.line,
                                    format!("`key={key}` isn't in names.toml"),
                                ),
                                Some(n) if n.kind != kind.class() => push(
                                    Severity::Error,
                                    file,
                                    b.line,
                                    format!(
                                        "`{key}` is a {} in names.toml, but tagged `.{}` here",
                                        n.kind,
                                        kind.class()
                                    ),
                                ),
                                _ => {}
                            }
                        }
                        Inline::Link { target, .. } => {
                            if let Some(id) = target.strip_prefix('#') {
                                if !ids.contains_key(id) {
                                    push(
                                        Severity::Error,
                                        file,
                                        b.line,
                                        format!("link to #{id}, which no block has"),
                                    );
                                }
                            }
                        }
                        Inline::SectionStart { reference } => {
                            check_prose_ref(
                                reference,
                                self.meta.refs.as_ref(),
                                file,
                                b.line,
                                &mut push,
                            );
                        }
                        Inline::PageBreak { edition: None, .. } if self.meta.edition.is_none() => {
                            push(
                                Severity::Warning,
                                file,
                                b.line,
                                "a source page, but book.toml has no [edition]".into(),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        for (key, n) in &self.names {
            if !matches!(n.kind.as_str(), "person" | "place" | "group" | "named") {
                push(
                    Severity::Error,
                    "names.toml",
                    0,
                    format!(
                        "[{key}]: kind \"{}\" isn't person, place, group or named",
                        n.kind
                    ),
                );
            }
            if !used.contains(key) {
                push(
                    Severity::Warning,
                    "names.toml",
                    0,
                    format!("[{key}] isn't used in the text"),
                );
            }
        }

        for (s, f, l, m) in found {
            self.report(s, &f, l, m);
        }
    }
}

fn id_looks_right(id: &str) -> bool {
    let Some((chapter, n)) = id.rsplit_once('-') else {
        return false;
    };
    !chapter.is_empty()
        && chapter
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && n.len() == 4
        && n.bytes().all(|b| b.is_ascii_digit())
}

/// A prose reference: the book's prefix, then one number per level.
fn check_prose_ref(
    r: &str,
    scheme: Option<&RefScheme>,
    file: &str,
    line: usize,
    push: &mut impl FnMut(Severity, &str, usize, String),
) {
    let Some(scheme) = scheme else {
        push(
            Severity::Error,
            file,
            line,
            format!("`{r}` needs a [refs] scheme in book.toml"),
        );
        return;
    };
    if !scheme.owns(r) {
        push(
            Severity::Error,
            file,
            line,
            format!(
                "`{r}` doesn't start with one of the book's works: {}",
                scheme.named()
            ),
        );
        return;
    }
    let numbers: Vec<&str> = r.split('.').skip(1).collect();
    if let Some(levels) = &scheme.levels {
        if numbers.len() != levels.len() || !numbers.iter().all(|n| n.parse::<u32>().is_ok()) {
            push(
                Severity::Error,
                file,
                line,
                format!(
                    "`{r}` should have {} numbers ({})",
                    levels.len(),
                    levels.join(".")
                ),
            );
        }
    }
}

fn number_blocks(
    blocks: &mut [Block],
    scheme: Option<&RefScheme>,
    file: &str,
    found: &mut Vec<(String, usize, String)>,
) {
    for b in blocks {
        let line = b.line;
        let reference = b.reference.clone();
        match &mut b.kind {
            BlockKind::Verse(v) | BlockKind::Stanza(v) => {
                if let (Some(r), Some(s)) = (&reference, scheme) {
                    number_verse(v, r, s, file, line, found);
                }
                number_blocks(&mut v.stanzas, scheme, file, found);
            }
            BlockKind::Speech { blocks, .. }
            | BlockKind::Epigraph(blocks)
            | BlockKind::Letter(blocks)
            | BlockKind::Quote(blocks)
            | BlockKind::Center(blocks)
            | BlockKind::Figure(blocks)
            | BlockKind::Note { blocks, .. } => number_blocks(blocks, scheme, file, found),
            _ => {}
        }
    }
}

fn number_verse(
    v: &mut Verse,
    r: &str,
    scheme: &RefScheme,
    file: &str,
    line: usize,
    found: &mut Vec<(String, usize, String)>,
) {
    let mut err = |m: String| found.push((file.to_string(), line, m));
    match scheme.unit.as_deref().unwrap_or("line") {
        "line" => {
            // `bw`, or a book or canto within the work: `od.1`, `inf.34`.
            if !scheme.owns(r) || !r.split('.').skip(1).all(|n| n.parse::<u32>().is_ok()) {
                err(format!(
                    "counted verse takes a work, or a work with book numbers (`od.1`): not `{r}`; the works are {}",
                    scheme.named()
                ));
                return;
            }
            let Some(mut n) = v.start else {
                if !v.lines.is_empty() {
                    err("counted verse needs `start=`: the number of its first line".into());
                }
                return;
            };
            for l in &mut v.lines {
                let (from, to) = line_override(&l.inlines).unwrap_or((n, n));
                l.reference = Some(if from == to {
                    format!("{r}.{from}")
                } else {
                    format!("{r}.{from}-{to}")
                });
                n = to + 1;
            }
        }
        unit @ ("stanza" | "stanza.line") => {
            if !scheme.owns(r) || !r.contains('.') {
                err(format!(
                    "`ref={r}` should be a work and a stanza number (`vsp.1`); the works are {}",
                    scheme.named()
                ));
                return;
            }
            if unit == "stanza.line" {
                for (k, l) in v.lines.iter_mut().enumerate() {
                    l.reference = Some(format!("{r}.{}", k + 1));
                }
            }
        }
        other => err(format!(
            "[refs] unit \"{other}\" isn't line, stanza or stanza.line"
        )),
    }
}

/// The original lines a verse line says it covers, `{n=12-13}`.
fn line_override(inlines: &[Inline]) -> Option<(u32, u32)> {
    let mut all = Vec::new();
    walk_inlines(inlines, &mut all);
    all.into_iter().find_map(|i| match i {
        Inline::Span {
            kind: SpanKind::LineOverride { from, to },
            ..
        } => Some((*from, *to)),
        _ => None,
    })
}
