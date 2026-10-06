//! A translation matched to its original by canonical reference (SPEC.md
//! §5, VOCABULARY.md, Alignment): line to line for line schemes, stanza to
//! stanza otherwise.

use std::collections::{BTreeMap, HashSet};

use crate::model::*;
use crate::{Book, Diagnostic, Severity};

/// One unit of the translation, with what it matches in the original.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    /// The translation's reference: `bw.4`, `bw.12-13`, `vsp.1`.
    pub reference: String,
    pub translation: Vec<String>,
    pub original: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Alignment {
    pub pairs: Vec<Pair>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A unit to align: its reference, the original references it covers, its
/// lines of text, and where it is.
struct Unit {
    reference: String,
    covers: Vec<String>,
    lines: Vec<String>,
    file: String,
    line: usize,
}

pub fn align(translation: &Book, original: &Book) -> Alignment {
    let mut out = Alignment::default();
    let mut report = |severity, file: &str, line, message: String| {
        out.diagnostics.push(Diagnostic {
            severity,
            file: file.into(),
            line,
            message,
        });
    };

    if translation.meta.original.as_deref() != Some(original.meta.id.as_str()) {
        report(
            Severity::Error,
            "book.toml",
            0,
            format!(
                "this book's original is {:?}, not `{}`",
                translation.meta.original, original.meta.id
            ),
        );
    }
    let (Some(ts), Some(os)) = (&translation.meta.refs, &original.meta.refs) else {
        report(
            Severity::Error,
            "book.toml",
            0,
            "both books need a [refs] scheme to align".into(),
        );
        return out;
    };
    if ts.unit != os.unit {
        report(
            Severity::Error,
            "book.toml",
            0,
            format!(
                "the units differ: {:?} here, {:?} in the original",
                ts.unit, os.unit
            ),
        );
        return out;
    }
    let mine_works: Vec<&str> = ts.prefixes();
    if let Some(w) = mine_works.iter().find(|w| !os.prefixes().contains(w)) {
        report(
            Severity::Error,
            "book.toml",
            0,
            format!("the original has no work `{w}`"),
        );
    }
    let by_line = ts.unit.as_deref().unwrap_or("line") == "line";

    let theirs: BTreeMap<String, Unit> = units(original, by_line)
        .into_iter()
        .map(|u| (u.reference.clone(), u))
        .collect();
    let mine = units(translation, by_line);
    let mut seen = HashSet::new();
    for u in &mine {
        let mut original_lines = Vec::new();
        for key in &u.covers {
            if !seen.insert(key.clone()) {
                report(
                    Severity::Warning,
                    &u.file,
                    u.line,
                    format!("{key} is matched twice"),
                );
            }
            match theirs.get(key) {
                Some(o) => original_lines.extend(o.lines.iter().cloned()),
                None => report(
                    Severity::Error,
                    &u.file,
                    u.line,
                    format!("{key} isn't in the original"),
                ),
            }
        }
        out.pairs.push(Pair {
            reference: u.reference.clone(),
            translation: u.lines.clone(),
            original: original_lines,
        });
    }
    // Only within works the translation has: an anthology may leave whole
    // works out.
    let works: HashSet<&str> = mine
        .iter()
        .filter_map(|u| u.reference.split('.').next())
        .collect();
    let mut skipped: Vec<&Unit> = theirs
        .values()
        .filter(|o| !seen.contains(&o.reference))
        .filter(|o| works.contains(o.reference.split('.').next().unwrap_or_default()))
        .collect();
    skipped.sort_by_key(|o| (o.file.clone(), o.line));
    for o in skipped {
        report(
            Severity::Warning,
            "",
            0,
            format!(
                "the translation skips {} ({}:{})",
                o.reference, o.file, o.line
            ),
        );
    }
    out
}

/// The book's units: each numbered line, or each stanza.
fn units(book: &Book, by_line: bool) -> Vec<Unit> {
    let mut out = Vec::new();
    for (file, b) in book.blocks() {
        let (BlockKind::Verse(v) | BlockKind::Stanza(v)) = &b.kind else {
            continue;
        };
        if by_line {
            for l in &v.lines {
                let Some(r) = &l.reference else { continue };
                out.push(Unit {
                    reference: r.clone(),
                    covers: expand(r),
                    lines: vec![plain_text(&l.inlines)],
                    file: file.into(),
                    line: l.line,
                });
            }
        } else if let (BlockKind::Stanza(_), Some(r)) = (&b.kind, &b.reference) {
            out.push(Unit {
                reference: r.clone(),
                covers: vec![r.clone()],
                lines: v.lines.iter().map(|l| plain_text(&l.inlines)).collect(),
                file: file.into(),
                line: b.line,
            });
        }
    }
    out
}

/// `bw.12-13` covers `bw.12` and `bw.13`.
fn expand(r: &str) -> Vec<String> {
    let Some((prefix, last)) = r.rsplit_once('.') else {
        return vec![r.into()];
    };
    match crate::parse::parse_range(last) {
        Some((from, to)) => (from..=to).map(|n| format!("{prefix}.{n}")).collect(),
        None => vec![r.into()],
    }
}
