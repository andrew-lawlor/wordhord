//! EPUB to Wordhord source, by recipe (SPEC.md §4, Import). The output is
//! read back with `wordhord-book` and compared line by line, so nothing is
//! lost to escaping without a report.

pub mod convert;
pub mod epub;
pub mod recipe;

use std::fmt::Write as _;
use std::path::Path;

use wordhord_book::model::{plain_text, BlockKind, Inline};
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
    let mut c = Converter::new(recipe)?;
    for doc in docs {
        let text = epub::xml_entities(&doc.text);
        let parsed = roxmltree::Document::parse_with_options(
            &text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .map_err(|e| format!("{}: {e}", doc.href))?;
        c.document(&parsed);
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

/// One chapter as Djot in the vocabulary.
pub fn chapter_source(number: usize, ch: &Chapter) -> String {
    let mut s = String::new();
    for (k, b) in ch.blocks.iter().enumerate() {
        let id = format!("ch{number:02}-{:04}", k + 1);
        match b {
            Out::Heading { level, inlines } => {
                let _ = writeln!(
                    s,
                    "{{#{id}}}\n{} {}\n",
                    "#".repeat(*level as usize),
                    line(inlines)
                );
            }
            Out::Paragraph(inlines) => {
                let _ = writeln!(s, "{{#{id}}}\n{}\n", line(inlines));
            }
            Out::Stanza { number, lines } => {
                match number {
                    Some(n) => {
                        let _ = writeln!(s, "{{#{id} ref=\"{}.{n}\"}}\n::: stanza", ch.prefix);
                    }
                    None => {
                        let _ = writeln!(s, "{{#{id}}}\n::: verse");
                    }
                }
                for l in lines {
                    let _ = writeln!(s, "{}{}", "  ".repeat(l.indent as usize), line(&l.inlines));
                }
                s.push_str(":::\n\n");
            }
            Out::Break => s.push_str("* * *\n\n"),
            Out::Commentary { on, blocks } => {
                // More colons than quoted verse's three, so it nests inside.
                let _ = writeln!(s, "{{#{id} on=\"{on}\"}}\n::::: commentary");
                let mut parts = Vec::new();
                for b in blocks {
                    match b {
                        Out::Paragraph(p) => parts.push(line(p)),
                        Out::Stanza { lines, .. } => {
                            let body: Vec<String> = lines
                                .iter()
                                .map(|l| {
                                    format!(
                                        "{}{}",
                                        "  ".repeat(l.indent as usize),
                                        line(&l.inlines)
                                    )
                                })
                                .collect();
                            parts.push(format!("::: verse\n{}\n:::", body.join("\n")));
                        }
                        _ => {}
                    }
                }
                let _ = writeln!(s, "{}\n:::::\n", parts.join("\n\n"));
            }
        }
    }
    s.push_str(&footnotes(ch));
    s
}

/// A chapter's footnotes, after its blocks. Later paragraphs of a note
/// are indented, so they stay in it.
fn footnotes(ch: &Chapter) -> String {
    let mut s = String::new();
    for (label, paragraphs) in &ch.notes {
        let body: Vec<String> = paragraphs
            .iter()
            .enumerate()
            .map(|(k, p)| {
                if k == 0 {
                    line(p)
                } else {
                    format!("  {}", line(p))
                }
            })
            .collect();
        let _ = writeln!(s, "[^{label}]: {}\n", body.join("\n\n"));
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

/// Reads the written book back and compares each block's text with what
/// was meant. Returns the differences.
pub fn verify(imported: &Imported, out: &Path) -> Result<(Book, Vec<String>), String> {
    let book = Book::load(out).map_err(|d| d.to_string())?;
    let mut problems = Vec::new();
    for (k, (ch, read)) in imported.chapters.iter().zip(&book.chapters).enumerate() {
        // Footnote definitions are compared below.
        let read_blocks: Vec<&wordhord_book::model::Block> = read
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
            match (b, &r.kind) {
                (Out::Break, BlockKind::SceneBreak) => {}
                (Out::Heading { inlines, .. }, BlockKind::Heading { text, .. }) => {
                    problems.extend(differ(&id, inlines, text))
                }
                (Out::Paragraph(i), BlockKind::Paragraph(t)) => problems.extend(differ(&id, i, t)),
                (Out::Stanza { lines, .. }, BlockKind::Stanza(v) | BlockKind::Verse(v)) => {
                    if lines.len() != v.lines.len() {
                        problems.push(format!(
                            "#{id}: wrote {} lines, read {}",
                            lines.len(),
                            v.lines.len()
                        ));
                    }
                    for (l, rl) in lines.iter().zip(&v.lines) {
                        problems.extend(differ(&id, &l.inlines, &rl.inlines));
                        if l.indent != rl.indent {
                            problems.push(format!(
                                "#{id}: wrote indent {}, read {}",
                                l.indent, rl.indent
                            ));
                        }
                    }
                }
                (Out::Commentary { blocks: meant, .. }, BlockKind::Commentary { blocks, .. }) => {
                    if meant.len() != blocks.len() {
                        problems.push(format!(
                            "#{id}: wrote {} parts of the note, read {}",
                            meant.len(),
                            blocks.len()
                        ));
                    }
                    for (m, rb) in meant.iter().zip(blocks) {
                        match (m, &rb.kind) {
                            (Out::Paragraph(p), BlockKind::Paragraph(t)) => {
                                problems.extend(differ(&id, p, t))
                            }
                            (Out::Stanza { lines, .. }, BlockKind::Verse(v)) => {
                                for (l, rl) in lines.iter().zip(&v.lines) {
                                    problems.extend(differ(&id, &l.inlines, &rl.inlines));
                                }
                            }
                            _ => problems
                                .push(format!("#{id}: part of the note read back differently")),
                        }
                    }
                }
                _ => problems.push(format!("#{id}: read back as a different kind of block")),
            }
        }
    }
    // Footnotes: the same labels, with the same paragraphs.
    for (ch, read) in imported.chapters.iter().zip(&book.chapters) {
        let got: Vec<(&String, &Vec<wordhord_book::model::Block>)> = read
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
        for ((label, paragraphs), (read_label, blocks)) in ch.notes.iter().zip(got) {
            if label != read_label {
                problems.push(format!(
                    "{}: wrote footnote [^{label}], read [^{read_label}]",
                    read.file
                ));
            }
            if paragraphs.len() != blocks.len() {
                problems.push(format!(
                    "{}: [^{label}] has {} paragraphs, read {}",
                    read.file,
                    paragraphs.len(),
                    blocks.len()
                ));
            }
            for (p, b) in paragraphs.iter().zip(blocks) {
                if let BlockKind::Paragraph(t) = &b.kind {
                    problems.extend(differ(&format!("[^{label}]"), p, t));
                }
            }
        }
    }
    Ok((book, problems))
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
