//! Djot source to the vocabulary's blocks and inlines. Anything outside the
//! vocabulary is reported, and its text kept, so later checks still see it.

use std::ops::Range;

use jotdown::{AttributeKind, Attributes, Container, Event, Parser};

use crate::model::*;
use crate::{Diagnostic, Severity};

/// Block-level `{…}` attributes, Djot's three kinds pulled apart.
#[derive(Default)]
struct Attrs {
    id: Option<String>,
    classes: Vec<String>,
    pairs: Vec<(String, String)>,
}

impl Attrs {
    fn of(a: &Attributes) -> Self {
        let mut out = Attrs::default();
        for (k, v) in a.iter() {
            match k {
                AttributeKind::Class => out
                    .classes
                    .extend(v.to_string().split_whitespace().map(String::from)),
                AttributeKind::Id => out.id = Some(v.to_string()),
                AttributeKind::Pair { key } => out.pairs.push((key.to_string(), v.to_string())),
                AttributeKind::Comment => {}
            }
        }
        out
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Parses one chapter file.
pub fn parse(src: &str, file: &str) -> (Vec<Block>, Vec<Diagnostic>) {
    let mut p = P {
        events: Parser::new(src).into_offset_iter().collect(),
        pos: 0,
        src,
        file: file.to_string(),
        diagnostics: Vec::new(),
        line_starts: std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect(),
        heading_id: None,
    };
    let mut blocks = Vec::new();
    while p.pos < p.events.len() {
        p.block(&mut blocks);
    }
    (blocks, p.diagnostics)
}

struct P<'s> {
    events: Vec<(Event<'s>, Range<usize>)>,
    pos: usize,
    src: &'s str,
    file: String,
    diagnostics: Vec<Diagnostic>,
    line_starts: Vec<usize>,
    /// A heading's ID: Djot puts `{#id}` before a heading on its section.
    heading_id: Option<String>,
}

const BLOCK_KEYS: &[&str] = &["id", "lang"];

impl<'s> P<'s> {
    fn line(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= offset)
    }

    fn report(&mut self, severity: Severity, offset: usize, message: impl Into<String>) {
        let line = self.line(offset);
        self.diagnostics.push(Diagnostic {
            severity,
            file: self.file.clone(),
            line,
            message: message.into(),
        });
    }

    fn next(&mut self) -> Option<(Event<'s>, Range<usize>)> {
        let e = self.events.get(self.pos).cloned();
        self.pos += 1;
        e
    }

    /// Blocks until the end of the current container.
    fn blocks(&mut self) -> Vec<Block> {
        let mut out = Vec::new();
        while let Some((e, _)) = self.events.get(self.pos) {
            if matches!(e, Event::End(_)) {
                self.pos += 1;
                break;
            }
            self.block(&mut out);
        }
        out
    }

    /// Skips the rest of a container, keeping nothing.
    fn skip(&mut self) {
        let mut depth = 1;
        while let Some((e, _)) = self.next() {
            match e {
                Event::Start(..) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// Reports attribute keys and classes a block can't have.
    fn allow(&mut self, at: &Attrs, keys: &[&str], classes: &[&str], what: &str, offset: usize) {
        if at.id.is_some() && !keys.contains(&"id") {
            self.report(Severity::Error, offset, format!("{what} can't have an #id"));
        }
        for (k, _) in at.pairs.clone() {
            if !keys.contains(&k.as_str()) {
                self.report(Severity::Error, offset, format!("{what} can't have `{k}=`"));
            }
        }
        for c in at.classes.clone() {
            if !classes.contains(&c.as_str()) {
                self.report(
                    Severity::Error,
                    offset,
                    format!("`.{c}` isn't in the vocabulary here ({what})"),
                );
            }
        }
        if let Some(lang) = at.get("lang") {
            self.check_lang(lang.to_string(), offset);
        }
    }

    fn check_lang(&mut self, lang: String, offset: usize) {
        if !(lang.len() == 3 && lang.bytes().all(|b| b.is_ascii_lowercase())) {
            self.report(
                Severity::Error,
                offset,
                format!("`lang={lang}` isn't an ISO 639-3 code (three lowercase letters)"),
            );
        }
    }

    fn new_block(&self, at: &Attrs, kind: BlockKind, offset: usize) -> Block {
        Block {
            id: at.id.clone(),
            kind,
            lang: at.get("lang").map(String::from),
            reference: at.get("ref").map(String::from),
            line: self.line(offset),
        }
    }

    fn block(&mut self, out: &mut Vec<Block>) {
        let Some((event, range)) = self.next() else {
            return;
        };
        let at = match &event {
            Event::Start(_, a) | Event::ThematicBreak(a) => Attrs::of(a),
            _ => Attrs::default(),
        };
        match event {
            Event::Blankline | Event::End(_) => {}
            Event::Attributes(_) => {
                self.report(
                    Severity::Warning,
                    range.start,
                    "attributes here aren't attached to any block",
                );
            }
            Event::ThematicBreak(_) => {
                self.allow(&at, &[], &[], "a scene break", range.start);
                out.push(self.new_block(&at, BlockKind::SceneBreak, range.start));
            }
            Event::Start(Container::Section { .. }, _) => {
                self.heading_id = at.id.clone();
                let inner = self.blocks();
                out.extend(inner);
            }
            Event::Start(Container::Heading { level, .. }, _) => {
                let mut at = at;
                if at.id.is_none() {
                    at.id = self.heading_id.take();
                }
                self.allow(&at, BLOCK_KEYS, &[], "a heading", range.start);
                let text = self.inlines();
                out.push(self.new_block(&at, BlockKind::Heading { level, text }, range.start));
            }
            Event::Start(Container::Paragraph, ref a) => {
                let read = !a.is_empty();
                self.unread_syntax(range.clone(), read);
                self.allow(&at, &["id", "lang", "ref"], &[], "a paragraph", range.start);
                let text = self.inlines();
                out.push(self.new_block(&at, BlockKind::Paragraph(text), range.start));
            }
            Event::Start(Container::Div { class }, _) => {
                let block = match class {
                    "verse" | "stanza" => self.verse(class, at, range.start),
                    "speech" | "epigraph" | "letter" | "center" | "figure" => {
                        let keys: &[&str] = if class == "speech" {
                            &["id", "lang", "speaker"]
                        } else {
                            BLOCK_KEYS
                        };
                        self.allow(&at, keys, &[], &format!("`::: {class}`"), range.start);
                        let blocks = self.blocks();
                        let kind = match class {
                            "speech" => BlockKind::Speech {
                                speaker: at.get("speaker").map(String::from),
                                blocks,
                            },
                            "epigraph" => BlockKind::Epigraph(blocks),
                            "letter" => BlockKind::Letter(blocks),
                            "center" => BlockKind::Center(blocks),
                            _ => BlockKind::Figure(blocks),
                        };
                        self.new_block(&at, kind, range.start)
                    }
                    "commentary" => {
                        self.allow(
                            &at,
                            &["id", "lang", "on"],
                            &[],
                            "`::: commentary`",
                            range.start,
                        );
                        let on = at.get("on").map(String::from);
                        if on.is_none() {
                            self.report(
                                Severity::Error,
                                range.start,
                                "a commentary needs `on=`: the passage it's about",
                            );
                        }
                        let blocks = self.blocks();
                        self.new_block(
                            &at,
                            BlockKind::Commentary {
                                on: on.unwrap_or_default(),
                                blocks,
                            },
                            range.start,
                        )
                    }
                    other => {
                        self.report(
                            Severity::Error,
                            range.start,
                            format!("`::: {other}` isn't in the vocabulary"),
                        );
                        let inner = self.blocks();
                        out.extend(inner);
                        return;
                    }
                };
                out.push(block);
            }
            Event::Start(Container::Blockquote, _) => {
                self.allow(&at, BLOCK_KEYS, &[], "a quotation", range.start);
                let blocks = self.blocks();
                out.push(self.new_block(&at, BlockKind::Quote(blocks), range.start));
            }
            Event::Start(Container::Footnote { label }, _) => {
                let blocks = self.blocks();
                out.push(self.new_block(
                    &at,
                    BlockKind::Note {
                        label: label.to_string(),
                        blocks,
                    },
                    range.start,
                ));
            }
            Event::Start(Container::Table, _) => {
                self.allow(&at, BLOCK_KEYS, &[], "a table", range.start);
                let rows = self.table();
                out.push(self.new_block(&at, BlockKind::Table(rows), range.start));
            }
            Event::Start(c, _) => {
                self.report(
                    Severity::Error,
                    range.start,
                    format!("{} isn't in the vocabulary", describe(&c)),
                );
                self.skip();
            }
            other => {
                self.report(
                    Severity::Error,
                    range.start,
                    format!("unexpected {other:?} between blocks"),
                );
            }
        }
    }

    /// Where the container just opened ends in the source. Djot gives a
    /// paragraph's start an empty range, so this scans to its end.
    fn container_end(&self) -> usize {
        let mut depth = 1;
        let mut end = self
            .events
            .get(self.pos.saturating_sub(1))
            .map_or(0, |(_, r)| r.end);
        for (e, r) in &self.events[self.pos.min(self.events.len())..] {
            end = end.max(r.end);
            match e {
                Event::Start(..) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        end
    }

    /// Catches Djot syntax that silently became text: an attribute line it
    /// couldn't read, or a `:::` that didn't open a block. `read` says Djot
    /// did read the block's attributes: then a first `{…}` line is them.
    fn unread_syntax(&mut self, range: Range<usize>, read: bool) {
        let end = self.container_end().max(range.end).min(self.src.len());
        let text = &self.src[range.start..end];
        let mut offset = range.start;
        for line in text.split_inclusive('\n') {
            let t = line.trim();
            if looks_like_attributes(t) && offset == range.start && !read {
                self.report(
                    Severity::Error,
                    offset,
                    "this attribute line wasn't read, so it's text: quote any value with a dot in it (ref=\"vsp.1\")",
                );
            } else if t.starts_with(":::") {
                self.report(
                    Severity::Error,
                    offset,
                    "this `:::` didn't open a block: is the attribute line above it valid?",
                );
            }
            offset += line.len();
        }
    }

    fn verse(&mut self, class: &str, at: Attrs, offset: usize) -> Block {
        // Lines are indented relative to the verse's own fence: inside a
        // footnote or a quotation, the container's indent isn't the poem's.
        let base = content_indent(self.line_at(offset));
        let keys: &[&str] = if class == "verse" {
            &["id", "lang", "ref", "start", "original", "indent"]
        } else {
            &["id", "lang", "ref", "original"]
        };
        self.allow(&at, keys, &[], &format!("`::: {class}`"), offset);
        let mut verse = Verse {
            original: at.get("original").map(String::from),
            ..Verse::default()
        };
        if let Some(s) = at.get("start") {
            match s.parse::<u32>() {
                Ok(n) if n > 0 => verse.start = Some(n),
                _ => self.report(
                    Severity::Error,
                    offset,
                    format!("`start={s}` isn't a line number"),
                ),
            }
        }
        if let Some(s) = at.get("indent") {
            match s.parse::<u8>() {
                Ok(n) => verse.indent = n,
                _ => self.report(
                    Severity::Error,
                    offset,
                    format!("`indent={s}` isn't a number of steps"),
                ),
            }
        }
        let mut group = 0;
        while let Some((e, r)) = self.next() {
            match e {
                Event::End(_) => break,
                Event::Blankline => {}
                Event::Start(Container::Paragraph, a) => {
                    if !a.is_empty() {
                        self.report(
                            Severity::Error,
                            r.start,
                            "lines of verse can't have attributes; put them on the verse",
                        );
                    }
                    self.unread_syntax(r.clone(), false);
                    for (inlines, first) in self.lines() {
                        verse.lines.push(VerseLine {
                            indent: self.indent_at(first, base),
                            group,
                            inlines: split_caesuras(inlines),
                            reference: None,
                            line: self.line(first),
                        });
                    }
                    group += 1;
                }
                Event::Start(Container::Div { class: "stanza" }, a) => {
                    let s = self.verse("stanza", Attrs::of(&a), r.start);
                    verse.stanzas.push(s);
                }
                Event::Start(c, _) => {
                    self.report(
                        Severity::Error,
                        r.start,
                        format!("{} can't go inside verse", describe(&c)),
                    );
                    self.skip();
                }
                _ => {}
            }
        }
        if !verse.lines.is_empty() && !verse.stanzas.is_empty() {
            self.report(
                Severity::Warning,
                offset,
                "verse has both loose lines and stanzas: the order between them is lost",
            );
        }
        let kind = if class == "verse" {
            BlockKind::Verse(verse)
        } else {
            BlockKind::Stanza(verse)
        };
        self.new_block(&at, kind, offset)
    }

    /// Leading spaces on the source line holding `offset`, as indent steps.
    /// Djot drops them; verse needs them.
    fn indent_at(&self, offset: usize, base: usize) -> u8 {
        let spaces = content_indent(self.line_at(offset));
        (spaces.saturating_sub(base) / 2).min(u8::MAX as usize) as u8
    }

    /// The source line holding `offset`.
    fn line_at(&self, offset: usize) -> &str {
        let start = self.src[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.src[start..]
            .find('\n')
            .map_or(self.src.len(), |i| start + i);
        &self.src[start..end]
    }

    /// A paragraph's inlines, split into source lines at its soft breaks,
    /// each with the offset where it starts.
    fn lines(&mut self) -> Vec<(Vec<Inline>, usize)> {
        let mut lines = Vec::new();
        let mut current = Vec::new();
        let mut first = None;
        while let Some((e, r)) = self.events.get(self.pos).cloned() {
            if matches!(e, Event::End(_)) {
                self.pos += 1;
                break;
            }
            if matches!(e, Event::Softbreak) {
                self.pos += 1;
                lines.push((
                    merge(std::mem::take(&mut current)),
                    first.take().unwrap_or(r.start),
                ));
                continue;
            }
            first.get_or_insert(r.start);
            if let Some(i) = self.inline() {
                current.push(i);
            }
        }
        if !current.is_empty() || first.is_some() {
            lines.push((merge(current), first.unwrap_or(0)));
        }
        lines
    }

    /// Inlines until the end of the current container.
    fn inlines(&mut self) -> Vec<Inline> {
        let mut out = Vec::new();
        loop {
            match self.events.get(self.pos) {
                None => break,
                Some((Event::End(_), _)) => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    if let Some(i) = self.inline() {
                        out.push(i);
                    }
                }
            }
        }
        merge(out)
    }

    fn inline(&mut self) -> Option<Inline> {
        let (e, r) = self.next()?;
        let text = |s: &str| Some(Inline::Text(s.to_string()));
        match e {
            Event::Str(s) => text(&s),
            Event::LeftSingleQuote => text("‘"),
            Event::RightSingleQuote => text("’"),
            Event::LeftDoubleQuote => text("“"),
            Event::RightDoubleQuote => text("”"),
            Event::Ellipsis => text("…"),
            Event::EnDash => text("–"),
            Event::EmDash => text("—"),
            Event::NonBreakingSpace => text("\u{a0}"),
            Event::Escape | Event::Blankline | Event::Attributes(_) => None,
            Event::Softbreak => Some(Inline::SoftBreak),
            Event::Hardbreak => Some(Inline::LineBreak),
            Event::FootnoteReference(label) => Some(Inline::NoteRef(label.to_string())),
            Event::Symbol(s) => {
                self.report(
                    Severity::Error,
                    r.start,
                    format!("`:{s}:` isn't in the vocabulary"),
                );
                Some(Inline::Text(format!(":{s}:")))
            }
            Event::Start(Container::Emphasis, _) => Some(Inline::Emphasis(self.inlines())),
            Event::Start(Container::Strong, _) => Some(Inline::Strong(self.inlines())),
            Event::Start(Container::Span, a) => self.span(Attrs::of(&a), r.start),
            Event::Start(Container::Link(target, _), _) => {
                let target = target.to_string();
                if !target.starts_with('#') {
                    self.report(
                        Severity::Error,
                        r.start,
                        format!("links go within the book (`#id`), not to `{target}`"),
                    );
                }
                Some(Inline::Link {
                    target,
                    inlines: self.inlines(),
                })
            }
            Event::Start(Container::Image(src, _), _) => Some(Inline::Image {
                src: src.to_string(),
                alt: self.inlines(),
                width: None,
                height: None,
            }),
            Event::Start(c, _) => {
                self.report(
                    Severity::Error,
                    r.start,
                    format!("{} isn't in the vocabulary", describe(&c)),
                );
                Some(Inline::Span {
                    kind: SpanKind::Foreign,
                    lang: None,
                    inlines: self.inlines(),
                })
            }
            Event::End(_) => None,
            other => {
                self.report(
                    Severity::Error,
                    r.start,
                    format!("unexpected {other:?} in text"),
                );
                None
            }
        }
    }

    fn span(&mut self, at: Attrs, offset: usize) -> Option<Inline> {
        let inlines = self.inlines();
        let kinds: Vec<String> = at
            .classes
            .iter()
            .filter(|c| {
                matches!(
                    c.as_str(),
                    "sc" | "person" | "place" | "group" | "named" | "gap" | "corr" | "pb" | "ref"
                )
            })
            .cloned()
            .collect();
        if kinds.len() > 1 {
            self.report(
                Severity::Error,
                offset,
                format!("a span can be only one of {}", kinds.join(", ")),
            );
        }
        let lang = at.get("lang").map(String::from);
        let kind = kinds.first().map(String::as_str);
        let empty = inlines.is_empty();
        let (keys, kind): (&[&str], Option<SpanKind>) = match kind {
            Some("pb") => {
                self.allow(&at, &["n", "ed"], &["pb"], "a source page", offset);
                if !empty {
                    self.report(
                        Severity::Error,
                        offset,
                        "a source page marker is empty: `[]{.pb n=24}`",
                    );
                }
                return match at.get("n") {
                    Some(n) => Some(Inline::PageBreak {
                        n: n.to_string(),
                        edition: at.get("ed").map(String::from),
                    }),
                    None => {
                        self.report(Severity::Error, offset, "a source page needs `n=`");
                        None
                    }
                };
            }
            Some("ref") => {
                self.allow(&at, &["n"], &["ref"], "a section start", offset);
                if !empty {
                    self.report(
                        Severity::Error,
                        offset,
                        "a section start is empty: `[]{.ref n=\"nh.12.42\"}`",
                    );
                }
                return match at.get("n") {
                    Some(n) => Some(Inline::SectionStart {
                        reference: n.to_string(),
                    }),
                    None => {
                        self.report(Severity::Error, offset, "a section start needs `n=`");
                        None
                    }
                };
            }
            Some("sc") => (&["lang"], Some(SpanKind::SmallCaps)),
            Some("gap") => (&[], Some(SpanKind::Gap)),
            Some("corr") => (&["lang"], Some(SpanKind::Corr)),
            Some(c @ ("person" | "place" | "group" | "named")) => {
                let kind = match c {
                    "person" => NameKind::Person,
                    "place" => NameKind::Place,
                    "group" => NameKind::Group,
                    _ => NameKind::Named,
                };
                if at.get("key").is_none() {
                    self.report(
                        Severity::Warning,
                        offset,
                        format!("a `.{c}` without `key=` can't be found across texts"),
                    );
                }
                let keys: &[&str] = if kind == NameKind::Named {
                    &["key", "type", "lang"]
                } else {
                    &["key", "lang"]
                };
                (
                    keys,
                    Some(SpanKind::Name {
                        kind,
                        key: at.get("key").map(String::from),
                        thing: at.get("type").map(String::from),
                    }),
                )
            }
            _ if at.get("n").is_some() => {
                let n = at.get("n").unwrap_or_default().to_string();
                match parse_range(&n) {
                    Some((from, to)) => (&["n"], Some(SpanKind::LineOverride { from, to })),
                    None => {
                        self.report(
                            Severity::Error,
                            offset,
                            format!("`n={n}` isn't a line or a range (`12`, `12-13`)"),
                        );
                        (&["n"], None)
                    }
                }
            }
            _ if lang.is_some() => (&["lang"], Some(SpanKind::Foreign)),
            _ => {
                self.report(
                    Severity::Error,
                    offset,
                    "this span has no meaning: give it a class or `lang=`",
                );
                (&[], None)
            }
        };
        let classes: Vec<&str> = kinds.iter().map(String::as_str).collect();
        self.allow(&at, keys, &classes, "a span", offset);
        Some(Inline::Span {
            kind: kind.unwrap_or(SpanKind::Foreign),
            lang,
            inlines,
        })
    }

    fn table(&mut self) -> Vec<Vec<Vec<Inline>>> {
        let mut rows = Vec::new();
        while let Some((e, r)) = self.next() {
            match e {
                Event::End(Container::Table) => break,
                Event::Start(Container::TableRow { .. }, _) => {
                    let mut cells = Vec::new();
                    while let Some((e, _)) = self.next() {
                        match e {
                            Event::Start(Container::TableCell { .. }, _) => {
                                cells.push(self.inlines())
                            }
                            Event::End(_) => break,
                            _ => {}
                        }
                    }
                    rows.push(cells);
                }
                Event::Start(Container::Caption, _) => {
                    self.report(Severity::Warning, r.start, "table captions aren't kept yet");
                    self.skip();
                }
                _ => {}
            }
        }
        rows
    }
}

fn describe(c: &Container) -> String {
    match c {
        Container::List { .. } => "a list".into(),
        Container::CodeBlock { .. } => "a code block".into(),
        Container::RawBlock { .. } | Container::RawInline { .. } => "raw output".into(),
        Container::DescriptionList => "a description list".into(),
        Container::LinkDefinition { .. } => "a link definition".into(),
        Container::Math { .. } => "math".into(),
        Container::Verbatim => "verbatim text".into(),
        other => format!("{other:?}"),
    }
}

/// A whole line that is one `{…}` attribute block: `{#id ref=x}`, not
/// text that happens to start with `{_italics_}` and end with `{.pb}`.
fn looks_like_attributes(line: &str) -> bool {
    let Some(inner) = line.strip_prefix('{').and_then(|l| l.strip_suffix('}')) else {
        return false;
    };
    inner.starts_with(|c: char| c == '#' || c == '.' || c.is_ascii_alphabetic())
        && !inner.contains(['{', '}'])
}

/// Spaces at the start of a line's content: after any footnote indent and
/// any `>` quotation markers (each with the one space that follows it).
fn content_indent(line: &str) -> usize {
    let lead = line.len() - line.trim_start_matches(' ').len();
    let mut rest = &line[lead..];
    if !rest.starts_with('>') {
        return lead;
    }
    loop {
        rest = &rest[1..];
        rest = rest.strip_prefix(' ').unwrap_or(rest);
        let spaces = rest.len() - rest.trim_start_matches(' ').len();
        if rest[spaces..].starts_with('>') {
            rest = &rest[spaces..];
        } else {
            return spaces;
        }
    }
}

/// `12` or `12-13`.
pub(crate) fn parse_range(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b) = (a.parse().ok()?, b.parse().ok()?);
    (a > 0 && b >= a).then_some((a, b))
}

/// Joins neighbouring text, so smart quotes and dashes don't split words.
fn merge(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for i in inlines {
        match (out.last_mut(), i) {
            (Some(Inline::Text(a)), Inline::Text(b)) => a.push_str(&b),
            (_, i) => out.push(i),
        }
    }
    out
}

/// ` | ` in a line of verse is a caesura.
fn split_caesuras(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::new();
    for i in inlines {
        match i {
            Inline::Text(t) if t.contains(" | ") => {
                let mut parts = t.split(" | ").peekable();
                while let Some(p) = parts.next() {
                    if !p.is_empty() {
                        out.push(Inline::Text(p.to_string()));
                    }
                    if parts.peek().is_some() {
                        out.push(Inline::Caesura);
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}
