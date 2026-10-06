//! EPUB to Wordhord source, by recipe (SPEC.md §4, Import). The output is
//! read back with `wordhord-book` and compared line by line, so nothing is
//! lost to escaping without a report.

pub mod convert;
pub mod epub;
pub mod recipe;

use std::fmt::Write as _;
use std::path::Path;

use wordhord_book::model::{plain_text, Block, BlockKind, Inline};
use wordhord_book::Book;

use convert::{Chapter, Converter, Out, Report};
pub use recipe::Recipe;

pub struct Imported {
    pub chapters: Vec<Chapter>,
    pub report: Report,
}

pub fn load_recipe(path: &Path) -> Result<Recipe, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))
}

pub fn import(epub: &Path, recipe: &Recipe) -> Result<Imported, String> {
    import_documents(epub::spine(epub)?, recipe)
}

/// Imports XHTML or XML documents in reading order. `import` gives it an
/// EPUB's spine; anything else that yields documents can too.
pub fn import_documents(docs: Vec<epub::Doc>, recipe: &Recipe) -> Result<Imported, String> {
    // Every document is parsed before the walk, so a note reference can
    // reach a note in a later file.
    let texts: Vec<(String, String)> = docs
        .into_iter()
        .map(|d| (d.href, epub::xml_entities(&d.text)))
        .collect();
    let mut parsed = Vec::with_capacity(texts.len());
    for (href, text) in &texts {
        let doc = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .map_err(|e| format!("{href}: {e}"))?;
        parsed.push(doc);
    }
    let mut c = Converter::new(recipe, &parsed)?;
    for k in 0..parsed.len() {
        c.document(k);
        if c.finished() {
            break;
        }
    }
    for w in &recipe.works {
        if !c.chapters.iter().any(|ch| ch.prefix == w.prefix) {
            c.report.problems.push(format!(
                "no heading \"{}\" found to start `{}`",
                w.start, w.prefix
            ));
        }
    }
    for ch in &c.chapters {
        check_numbers(ch, &mut c.report);
    }
    Ok(Imported {
        chapters: c.chapters,
        report: c.report,
    })
}

/// Stanza numbers should run 1, 2, 3… in each work.
fn check_numbers(ch: &Chapter, report: &mut Report) {
    let mut last = 0;
    for b in &ch.blocks {
        if let Out::Stanza {
            number: Some(n), ..
        } = b
        {
            // The same number again continues the stanza after a speaker.
            if *n != last + 1 && *n != last {
                report
                    .problems
                    .push(format!("{}: stanza {n} follows stanza {last}", ch.prefix));
            }
            last = *n;
        }
    }
}

/// Writes `book.toml` and `text/NN-prefix.dj` into a new or empty folder.
pub fn write(imported: &Imported, recipe: &Recipe, out: &Path) -> Result<(), String> {
    if out.exists()
        && std::fs::read_dir(out)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} isn't empty: import into a new folder",
            out.display()
        ));
    }
    std::fs::create_dir_all(out.join("text")).map_err(|e| e.to_string())?;
    let toml = toml::to_string(&recipe.book).map_err(|e| e.to_string())?;
    std::fs::write(out.join("book.toml"), toml).map_err(|e| e.to_string())?;
    for (k, ch) in imported.chapters.iter().enumerate() {
        let path = out
            .join("text")
            .join(format!("{:02}-{}.dj", k + 1, ch.prefix));
        std::fs::write(&path, chapter_source(k + 1, ch))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// One chapter as Djot in the vocabulary: its blocks, each with an ID,
/// then its footnotes.
pub fn chapter_source(number: usize, ch: &Chapter) -> String {
    let mut s = String::new();
    for (k, b) in ch.blocks.iter().enumerate() {
        let id = format!("ch{number:02}-{:04}", k + 1);
        s.push_str(&block(b, Some((&id, &ch.prefix))));
        s.push_str("\n\n");
    }
    s.push_str(&footnotes(ch));
    s
}

/// Nested blocks (in a quotation, a note, a speech), one string each.
/// Djot reads two tables with only a blank line between them as one, so a
/// second table gets an empty attribute line `{}`.
fn nested(blocks: &[Out]) -> Vec<String> {
    blocks
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let part = block(b, None);
            let after_table = k > 0 && matches!(blocks[k - 1], Out::Table(_));
            if after_table && matches!(b, Out::Table(_)) {
                format!("{{}}\n{part}")
            } else {
                part
            }
        })
        .collect()
}

/// One block as Djot. A top-level block gets its ID (and its reference);
/// a nested one, in a quotation or a note, gets neither.
fn block(b: &Out, top: Option<(&str, &str)>) -> String {
    let attrs = |extra: String| match top {
        Some((id, _)) => format!("{{#{id}{extra}}}\n"),
        None => String::new(),
    };
    let verse_lines = |lines: &[convert::Line]| -> Vec<String> {
        lines
            .iter()
            .map(|l| format!("{}{}", "  ".repeat(l.indent as usize), line(&l.inlines)))
            .collect()
    };
    match b {
        Out::Heading { level, inlines } => {
            format!(
                "{}{} {}",
                attrs(String::new()),
                "#".repeat(*level as usize),
                line(inlines)
            )
        }
        Out::Paragraph(inlines) => format!("{}{}", attrs(String::new()), line(inlines)),
        Out::Stanza { number, lines } => {
            let head = match (number, top) {
                (Some(n), Some((_, prefix))) => {
                    format!("{}::: stanza", attrs(format!(" ref=\"{prefix}.{n}\"")))
                }
                _ => format!("{}::: verse", attrs(String::new())),
            };
            format!("{head}\n{}\n:::", verse_lines(lines).join("\n"))
        }
        Out::Verse {
            reference,
            start,
            groups,
        } => {
            let mut extra = String::new();
            if let Some(r) = reference {
                let _ = write!(extra, " ref=\"{r}\"");
            }
            if let Some(n) = start {
                let _ = write!(extra, " start={n}");
            }
            let body: Vec<String> = groups.iter().map(|g| verse_lines(g).join("\n")).collect();
            format!("{}::: verse\n{}\n:::", attrs(extra), body.join("\n\n"))
        }
        Out::Break => "* * *".to_string(),
        Out::Commentary { on, blocks } => {
            // More colons than quoted verse's three, so it nests inside.
            let parts = nested(blocks);
            format!(
                "{}::::: commentary\n{}\n:::::",
                attrs(format!(" on=\"{on}\"")),
                parts.join("\n\n")
            )
        }
        Out::Quote(blocks) => {
            let parts = nested(blocks);
            let quoted: Vec<String> = parts
                .join("\n\n")
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect();
            format!("{}{}", attrs(String::new()), quoted.join("\n"))
        }
        Out::Speech { speaker, blocks } => {
            let parts = nested(blocks);
            let extra = speaker
                .as_ref()
                .map(|s| format!(" speaker=\"{s}\""))
                .unwrap_or_default();
            // When nested, the speaker still needs its attribute line.
            let head = match top {
                Some(_) => attrs(extra),
                None if speaker.is_some() => format!("{{{}}}\n", extra.trim_start()),
                None => String::new(),
            };
            // Four colons, so verse's three nest inside.
            format!("{head}:::: speech\n{}\n::::", parts.join("\n\n"))
        }
        Out::Table(rows) => {
            let lines: Vec<String> = rows
                .iter()
                .map(|r| {
                    let cells: Vec<String> = r.iter().map(|c| line(c)).collect();
                    format!("| {} |", cells.join(" | "))
                })
                .collect();
            format!("{}{}", attrs(String::new()), lines.join("\n"))
        }
    }
}

/// A chapter's footnotes, after its blocks. A note's first paragraph
/// follows its label; everything after is indented, so it stays in it.
fn footnotes(ch: &Chapter) -> String {
    let mut s = String::new();
    for (label, body) in &ch.notes {
        let parts = nested(body);
        let indent = |p: &String| -> String {
            p.lines()
                .map(|l| {
                    if l.is_empty() {
                        String::new()
                    } else {
                        format!("  {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let first_is_paragraph = matches!(body.first(), Some(Out::Paragraph(_)));
        let text = match parts.split_first() {
            Some((first, rest)) if first_is_paragraph => {
                let mut all = vec![first.clone()];
                all.extend(rest.iter().map(indent));
                all.join("\n\n")
            }
            _ => format!(
                "\n\n{}",
                parts.iter().map(indent).collect::<Vec<_>>().join("\n\n")
            ),
        };
        let _ = writeln!(s, "[^{label}]: {}\n", text);
    }
    s
}

/// Inlines as one line of Djot, escaped so they read back as written.
fn line(inlines: &[Inline]) -> String {
    let mut s = String::new();
    write_inlines(&mut s, inlines);
    escape_line_start(&s)
}

fn write_inlines(s: &mut String, inlines: &[Inline]) {
    for i in inlines {
        match i {
            Inline::Text(t) => s.push_str(&escape(t)),
            Inline::Emphasis(c) => {
                s.push_str("{_");
                write_inlines(s, c);
                s.push_str("_}");
            }
            Inline::Strong(c) => {
                s.push_str("{*");
                write_inlines(s, c);
                s.push_str("*}");
            }
            Inline::Caesura => s.push_str(" | "),
            Inline::PageBreak { n, edition } => {
                let _ = match edition {
                    Some(e) => write!(s, "[]{{.pb n=\"{n}\" ed=\"{e}\"}}"),
                    None => write!(s, "[]{{.pb n=\"{n}\"}}"),
                };
            }
            Inline::LineBreak | Inline::SoftBreak => s.push(' '),
            Inline::NoteRef(label) => {
                let _ = write!(s, "[^{label}]");
            }
            Inline::Span {
                kind: wordhord_book::model::SpanKind::Corr,
                inlines,
                ..
            } => {
                s.push('[');
                write_inlines(s, inlines);
                s.push_str("]{.corr}");
            }
            Inline::Span {
                kind: wordhord_book::model::SpanKind::Gap,
                inlines,
                ..
            } => {
                s.push('[');
                write_inlines(s, inlines);
                s.push_str("]{.gap}");
            }
            Inline::Span {
                kind: wordhord_book::model::SpanKind::Foreign,
                lang: Some(lang),
                inlines,
            } => {
                s.push('[');
                write_inlines(s, inlines);
                let _ = write!(s, "]{{lang={lang}}}");
            }
            other => {
                let _ = write!(s, "{}", escape(&plain_text(std::slice::from_ref(other))));
            }
        }
    }
}

/// Backslashes before anything Djot would read as markup.
fn escape(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    for (k, &c) in chars.iter().enumerate() {
        let next = chars.get(k + 1).copied();
        let markup = match c {
            '\\' | '_' | '*' | '[' | ']' | '{' | '}' | '`' | '^' | '~' | '$' | '<' | '|' => true,
            // A symbol, :name:.
            ':' => next.is_some_and(|n| n.is_alphanumeric()),
            // -- and --- become dashes; ... an ellipsis.
            '-' => next == Some('-'),
            // ![ is an image; a page marker may follow this text.
            '!' => next.is_none_or(|n| n == '['),
            '.' => next == Some('.') && chars.get(k + 2) == Some(&'.'),
            _ => false,
        };
        if markup {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A line that would start a heading, list, quotation or definition.
/// Ordered list markers are `1.`, `1)`, `(1)`, `a.`, `(a)`, `IV.`, `(iv)`
/// and the like, followed by a space or the end of the line.
fn escape_line_start(s: &str) -> String {
    let trimmed = s.trim_start();
    let lead = &s[..s.len() - trimmed.len()];
    let mut chars = trimmed.chars();
    let first = chars.next();
    let second = chars.next();
    match first {
        Some('#' | '>') => return format!("{lead}\\{trimmed}"),
        Some('-' | '+' | ':') if second.is_none_or(|c| c == ' ') => {
            return format!("{lead}\\{trimmed}")
        }
        _ => {}
    }
    let marker = regex::Regex::new(r"^(\()?([0-9]+|[A-Za-z]+)([.)])(\s|$)").expect("valid");
    match marker.captures(trimmed) {
        // `(I)`: escaping the parenthesis stops it opening a list.
        Some(c) if c.get(1).is_some() => format!("{lead}\\{trimmed}"),
        // `1.` or `IV)`: escaping the punctuation does.
        Some(c) => {
            let at = c.get(3).expect("matched").start();
            format!("{lead}{}\\{}", &trimmed[..at], &trimmed[at..])
        }
        None => s.to_string(),
    }
}

/// Reads the written book back and compares each block with what was
/// meant: its kind, its text, and the markers text can't show. Returns
/// the differences.
pub fn verify(imported: &Imported, out: &Path) -> Result<(Book, Vec<String>), String> {
    let book = Book::load(out).map_err(|d| d.to_string())?;
    let mut problems = Vec::new();
    for (k, (ch, read)) in imported.chapters.iter().zip(&book.chapters).enumerate() {
        // Footnote definitions are compared below.
        let read_blocks: Vec<&Block> = read
            .blocks
            .iter()
            .filter(|b| !matches!(b.kind, BlockKind::Note { .. }))
            .collect();
        if ch.blocks.len() != read_blocks.len() {
            problems.push(format!(
                "chapter {}: wrote {} blocks, read {}",
                k + 1,
                ch.blocks.len(),
                read_blocks.len()
            ));
        }
        for (b, r) in ch.blocks.iter().zip(read_blocks) {
            let id = r.id.clone().unwrap_or_default();
            compare(b, r, &id, &mut problems);
        }
        // Footnotes: the same labels, with the same contents.
        let got: Vec<(&String, &Vec<Block>)> = read
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Note { label, blocks } => Some((label, blocks)),
                _ => None,
            })
            .collect();
        if got.len() != ch.notes.len() {
            problems.push(format!(
                "{}: wrote {} footnotes, read {}",
                read.file,
                ch.notes.len(),
                got.len()
            ));
        }
        for ((label, body), (read_label, blocks)) in ch.notes.iter().zip(got) {
            if label != read_label {
                problems.push(format!(
                    "{}: wrote footnote [^{label}], read [^{read_label}]",
                    read.file
                ));
            }
            compare_all(
                body,
                blocks,
                &format!("{} [^{label}]", read.file),
                &mut problems,
            );
        }
    }
    Ok((book, problems))
}

fn compare_all(meant: &[Out], got: &[Block], id: &str, problems: &mut Vec<String>) {
    if meant.len() != got.len() {
        problems.push(format!(
            "{id}: wrote {} blocks, read {}",
            meant.len(),
            got.len()
        ));
    }
    for (m, g) in meant.iter().zip(got) {
        compare(m, g, id, problems);
    }
}

fn compare(meant: &Out, got: &Block, id: &str, problems: &mut Vec<String>) {
    let lines_match = |problems: &mut Vec<String>,
                       lines: Vec<(usize, &convert::Line)>,
                       read: &[wordhord_book::model::VerseLine]| {
        if lines.len() != read.len() {
            problems.push(format!(
                "{id}: wrote {} lines, read {}",
                lines.len(),
                read.len()
            ));
        }
        for ((group, l), rl) in lines.into_iter().zip(read) {
            problems.extend(differ(id, &l.inlines, &rl.inlines));
            if l.indent != rl.indent {
                problems.push(format!(
                    "{id}: wrote indent {}, read {}",
                    l.indent, rl.indent
                ));
            }
            if group != rl.group {
                problems.push(format!(
                    "{id}: wrote a line in group {group}, read group {}",
                    rl.group
                ));
            }
        }
    };
    match (meant, &got.kind) {
        (Out::Break, BlockKind::SceneBreak) => {}
        (Out::Heading { inlines, .. }, BlockKind::Heading { text, .. }) => {
            problems.extend(differ(id, inlines, text))
        }
        (Out::Paragraph(i), BlockKind::Paragraph(t)) => problems.extend(differ(id, i, t)),
        (Out::Stanza { lines, .. }, BlockKind::Stanza(v) | BlockKind::Verse(v)) => {
            lines_match(problems, lines.iter().map(|l| (0, l)).collect(), &v.lines)
        }
        (
            Out::Verse {
                reference,
                start,
                groups,
            },
            BlockKind::Verse(v),
        ) => {
            let lines = groups
                .iter()
                .enumerate()
                .flat_map(|(g, ls)| ls.iter().map(move |l| (g, l)))
                .collect();
            lines_match(problems, lines, &v.lines);
            if reference.is_some() && (got.reference != *reference || v.start != *start) {
                problems.push(format!(
                    "{id}: wrote ref {reference:?} start {start:?}, read {:?} {:?}",
                    got.reference, v.start
                ));
            }
        }
        (Out::Commentary { blocks: m, .. }, BlockKind::Commentary { blocks, .. }) => {
            compare_all(m, blocks, id, problems)
        }
        (Out::Quote(m), BlockKind::Quote(blocks)) => compare_all(m, blocks, id, problems),
        (Out::Speech { speaker, blocks: m }, BlockKind::Speech { speaker: s, blocks }) => {
            if speaker != s {
                problems.push(format!("{id}: wrote speaker {speaker:?}, read {s:?}"));
            }
            compare_all(m, blocks, id, problems)
        }
        (Out::Table(rows), BlockKind::Table(read)) => {
            if rows.len() != read.len() {
                problems.push(format!(
                    "{id}: wrote {} rows, read {}",
                    rows.len(),
                    read.len()
                ));
            }
            for (r, rr) in rows.iter().zip(read) {
                if r.len() != rr.len() {
                    problems.push(format!("{id}: wrote {} cells, read {}", r.len(), rr.len()));
                }
                for (c, rc) in r.iter().zip(rr) {
                    problems.extend(differ(id, c, rc));
                }
            }
        }
        _ => problems.push(format!("{id}: read back as a different kind of block")),
    }
}

/// The difference between what was written and what was read, if any:
/// the text, or the markers in it (caesuras, source pages), which the text
/// alone can't show. An escaped `\|` reads back as the same text as a
/// caesura, but it isn't one.
fn differ(id: &str, meant: &[Inline], got: &[Inline]) -> Option<String> {
    let (m, g) = (
        same_quotes(&plain_text(meant)),
        same_quotes(&plain_text(got)),
    );
    if m != g {
        return Some(format!("#{id}: wrote \"{m}\", read \"{g}\""));
    }
    let (mm, gm) = (markers(meant), markers(got));
    (mm != gm).then(|| format!("#{id}: wrote markers {mm:?}, read {gm:?}"))
}

/// Caesuras and source pages, in order.
fn markers(inlines: &[Inline]) -> Vec<String> {
    let mut all = Vec::new();
    wordhord_book::model::walk_inlines(inlines, &mut all);
    all.into_iter()
        .filter_map(|i| match i {
            Inline::Caesura => Some("|".to_string()),
            Inline::PageBreak { n, .. } => Some(format!("p{n}")),
            Inline::NoteRef(label) => Some(format!("^{label}")),
            Inline::Span {
                lang: Some(lang), ..
            } => Some(format!("lang={lang}")),
            _ => None,
        })
        .collect()
}

/// Djot makes straight quotes curly; that's not a difference.
fn same_quotes(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '‘' | '’' => '\'',
            '“' | '”' => '"',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_reads_back_as_written() {
        let samples = [
            "Odin's *ravens* and [brackets] {braces} _under_ a|b",
            "1. Not a list",
            "- Not a list either",
            "# Not a heading",
            "> Not a quote",
            "Time 6:30, :smile: and -- and ... dots",
            "Back\\slash, `tick`, ^caret^, ~tilde~, $dollar, <angle>",
            "(I)",
            "(iv) a numbered aside",
            "A. Smith",
            "IV) four",
            "May she ever rot!",
        ];
        for s in samples {
            let src = format!("{{#ch01-0001}}\n{}\n", line(&[Inline::Text(s.into())]));
            let events: String = jotdown::Parser::new(&src)
                .filter_map(|e| match e {
                    jotdown::Event::Str(t) => Some(t.to_string()),
                    jotdown::Event::RightSingleQuote => Some("'".into()),
                    _ => None,
                })
                .collect();
            assert_eq!(events, s, "{src}");
        }
    }
}
