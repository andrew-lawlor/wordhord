//! A chapter's XHTML as Wordhord's blocks, read through its CSS.
//!
//! The walk finds leaves: block elements with no blocks inside, each a
//! paragraph, a heading or a line of verse. Containers are walked into.
//! Then leaves are grouped: runs of verse lines into verse, numbered
//! stanzas where a line starts with a number, the rest by their style.
//!
//! Verse is recognised three ways: a hanging indent on short lines (Wilson's
//! *Odyssey*, Alexander's *Beowulf*); an ancestor called verse or poem
//! (Standard Ebooks); or a run of three or more short lines with no
//! first-line indent (Crawford's *Edda*). Prose paragraphs, even short ones,
//! have an indent, so dialogue stays prose.
//!
//! Notes are found before the walk. A link marked as a note reference is
//! one; so is a short link to a fragment that's set as a superscript or
//! classed as a note. Its target becomes the note, without its number or
//! its link back, and is left out of the chapter's own flow.

use std::collections::{HashMap, HashSet};

use roxmltree::{Node, NodeId};
use wordhord_book::model::{plain_text, Block, BlockKind, Inline, SpanKind, Verse, VerseLine};

use crate::css::{Align, Display, Style, Stylesheet};
use crate::package::{resolve, Epub};
use crate::xml;

/// Lines longer than this, in characters, are prose.
const SHORT_LINE: usize = 90;

/// A chapter's blocks, its notes at the end.
pub fn chapter(epub: &Epub, path: &str) -> Result<Vec<Block>, String> {
    let text = epub.text(path)?;
    let doc = xml::parse(&text).map_err(|e| format!("{path}: {e}"))?;
    let sheet = Stylesheet::new(&epub.stylesheets(path, &doc));
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name("body"))
        .ok_or_else(|| format!("{path}: no body"))?;
    let mut cx = Converter {
        epub,
        path: path.to_string(),
        sheet: &sheet,
        labels: HashMap::new(),
        skip: HashSet::new(),
        notes: Vec::new(),
        next_id: 0,
    };
    cx.find_notes(body);
    let mut pieces = Vec::new();
    let style = root_style(&sheet, body);
    cx.walk(body, &style, false, &mut pieces);
    let mut blocks = group(pieces, &mut cx.next_id);
    for note in std::mem::take(&mut cx.notes) {
        blocks.push(note);
    }
    Ok(blocks)
}

/// The style the body's children inherit: the cascade from `<html>` down.
fn root_style(sheet: &Stylesheet, body: Node) -> Style {
    let mut chain: Vec<Node> = body.ancestors().filter(|n| n.is_element()).collect();
    chain.reverse();
    let mut style = Style::default();
    for n in chain {
        style = sheet.style(n, &style);
    }
    style
}

struct Converter<'e, 's> {
    epub: &'e Epub,
    path: String,
    sheet: &'s Stylesheet,
    /// Note targets (`file#id`) and the labels their markers show.
    labels: HashMap<String, String>,
    /// Elements in this chapter that are note bodies: not in the flow.
    skip: HashSet<String>,
    notes: Vec<Block>,
    next_id: usize,
}

/// A leaf, or a block already made.
enum Piece {
    Leaf(Leaf),
    Block(Block),
}

struct Leaf {
    inlines: Vec<Inline>,
    style: Style,
    tag: String,
    id: Option<String>,
    lang: Option<String>,
    /// The element this leaf's lines belong to: a new one starts a new
    /// group of lines.
    parent: NodeId,
    /// Inside an element called verse or poem.
    in_verse: bool,
}

impl Leaf {
    fn text(&self) -> String {
        plain_text(&self.inlines)
    }

    /// The first line's start, in ems from the block's left edge.
    fn start(&self) -> f32 {
        self.style.left().em + self.style.text_indent.em
    }

    fn hanging(&self) -> bool {
        self.style.text_indent.em < -0.3 && self.style.left().em > 0.3
    }
}

impl<'e, 's> Converter<'e, 's> {
    fn id(&mut self) -> String {
        self.next_id += 1;
        format!("b{}", self.next_id)
    }

    // ---- notes ------------------------------------------------------

    fn find_notes(&mut self, body: Node) {
        let mut used: HashMap<String, String> = HashMap::new();
        // Each note's label, the file and fragment it's in, and the id of
        // the marker that points at it.
        let mut wanted: Vec<(String, String, String, Option<String>)> = Vec::new();
        for a in body.descendants().filter(|n| n.has_tag_name("a")) {
            let Some(href) = a.attribute("href") else {
                continue;
            };
            if !href.contains('#') || !is_noteref(a) {
                continue;
            }
            let target = resolve(&self.path, href);
            if self.labels.contains_key(&target) {
                continue;
            }
            let (file, fragment) = target.split_once('#').unwrap_or((&target, ""));
            // Links into the contents are navigation, not notes.
            let name = file.rsplit('/').next().unwrap_or(file).to_ascii_lowercase();
            let nav = self
                .epub
                .item(file)
                .is_some_and(|i| i.properties.iter().any(|p| p == "nav"))
                || name.contains("toc")
                || name.contains("contents");
            if nav {
                continue;
            }
            let mut label: String = text_of(a)
                .trim()
                .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')'))
                .to_string();
            if label.is_empty() {
                label = "*".to_string();
            }
            // Two notes with one label in a chapter: keep them apart.
            while used.get(&label).is_some_and(|t| *t != target) {
                label.push('′');
            }
            used.insert(label.clone(), target.clone());
            self.labels.insert(target.clone(), label.clone());
            if file == self.path.as_str() {
                self.skip.insert(fragment.to_string());
            }
            wanted.push((
                label,
                file.to_string(),
                fragment.to_string(),
                a.attribute("id").map(str::to_string),
            ));
        }
        // Each file of notes is read once.
        let mut files: Vec<&str> = wanted.iter().map(|w| w.1.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        let mut found: HashMap<String, Vec<Block>> = HashMap::new();
        for file in files {
            let Ok(text) = self.epub.text(file) else {
                continue;
            };
            let Ok(doc) = xml::parse(&text) else { continue };
            let own = (file != self.path.as_str())
                .then(|| Stylesheet::new(&self.epub.stylesheets(file, &doc)));
            let sheet = own.as_ref().unwrap_or(self.sheet);
            for (label, f, fragment, marker) in &wanted {
                if f != file {
                    continue;
                }
                if let Some(blocks) = self.note_body(&doc, sheet, file, fragment, marker.as_deref())
                {
                    found.insert(label.clone(), blocks);
                }
            }
        }
        for (label, ..) in wanted {
            if let Some(blocks) = found.remove(&label) {
                self.notes.push(Block {
                    id: Some(format!("note-{label}")),
                    kind: BlockKind::Note { label, blocks },
                    lang: None,
                    reference: None,
                    line: 0,
                });
            }
        }
    }

    /// The note at `fragment` in `doc`, as blocks, without its number or
    /// its link back to the marker.
    fn note_body(
        &mut self,
        doc: &roxmltree::Document,
        sheet: &Stylesheet,
        file: &str,
        fragment: &str,
        marker: Option<&str>,
    ) -> Option<Vec<Block>> {
        let target = doc
            .descendants()
            .find(|n| n.attribute("id") == Some(fragment))?;
        // An anchor inside a paragraph means the paragraph.
        let body = if is_block(root_style(sheet, target).display) {
            target
        } else {
            target
                .ancestors()
                .skip(1)
                .find(|a| a.is_element() && is_block(root_style(sheet, *a).display))?
        };
        let back = marker.map(|id| format!("#{id}"));
        let mut note = Converter {
            epub: self.epub,
            path: file.to_string(),
            sheet,
            labels: HashMap::new(),
            skip: HashSet::new(),
            notes: Vec::new(),
            next_id: self.next_id,
        };
        let mut pieces = Vec::new();
        let body_style = root_style(sheet, body);
        if note.has_blocks(body, &body_style) {
            note.walk(body, &body_style, false, &mut pieces);
        } else {
            let inlines = note.inlines(body, &body_style);
            pieces.push(Piece::Leaf(Leaf {
                inlines,
                style: body_style,
                tag: "p".into(),
                id: None,
                lang: None,
                parent: body.id(),
                in_verse: false,
            }));
        }
        self.next_id = note.next_id + 1000;
        let mut blocks = group(pieces, &mut self.next_id);
        strip_note_furniture(&mut blocks, target, back.as_deref());
        (!blocks.is_empty()).then_some(blocks)
    }

    // ---- the walk ----------------------------------------------------

    fn has_blocks(&self, node: Node, style: &Style) -> bool {
        node.children().filter(|c| c.is_element()).any(|c| {
            let s = self.sheet.style(c, style);
            (is_block(s.display) && !is_note_body(c))
                || (s.display != Display::None && self.has_blocks(c, &s))
        })
    }

    fn walk(&mut self, node: Node, style: &Style, in_verse: bool, out: &mut Vec<Piece>) {
        let mut run: Vec<Node> = Vec::new();
        for child in node.children() {
            if child.is_text() {
                run.push(child);
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let s = self.sheet.style(child, style);
            if s.display == Display::None || self.skipped(child) {
                continue;
            }
            if is_block(s.display) || self.has_blocks(child, &s) {
                self.flush(node, style, in_verse, &mut run, out);
                self.block(child, &s, in_verse, out);
            } else {
                run.push(child);
            }
        }
        self.flush(node, style, in_verse, &mut run, out);
    }

    fn skipped(&self, node: Node) -> bool {
        is_note_body(node)
            || node
                .attribute("id")
                .is_some_and(|id| self.skip.contains(id))
    }

    /// Inline content between blocks: a leaf of its own.
    fn flush(
        &mut self,
        parent: Node,
        style: &Style,
        in_verse: bool,
        run: &mut Vec<Node>,
        out: &mut Vec<Piece>,
    ) {
        if run.is_empty() {
            return;
        }
        let mut inlines = Vec::new();
        for n in run.drain(..) {
            self.inline_node(n, style, &mut inlines);
        }
        trim(&mut inlines);
        if inlines.is_empty() {
            return;
        }
        out.push(Piece::Leaf(Leaf {
            inlines,
            style: Style {
                display: Display::Block,
                ..style.clone()
            },
            tag: "p".into(),
            id: None,
            lang: None,
            parent: parent.id(),
            in_verse,
        }));
    }

    fn block(&mut self, node: Node, style: &Style, in_verse: bool, out: &mut Vec<Piece>) {
        let tag = node.tag_name().name().to_ascii_lowercase();
        let id = node.attribute("id").map(str::to_string);
        let lang = lang_of(node);
        let verse_here = in_verse || names_verse(node);
        let make = |kind: BlockKind, cx: &mut Self, out: &mut Vec<Piece>| {
            let id = id.clone().or_else(|| Some(cx.id()));
            out.push(Piece::Block(Block {
                id,
                kind,
                lang: lang.clone(),
                reference: None,
                line: 0,
            }));
        };
        match tag.as_str() {
            "hr" => make(BlockKind::SceneBreak, self, out),
            "pre" => make(BlockKind::Code(pre_text(node)), self, out),
            "ul" | "ol" => {
                let start = node
                    .attribute("start")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                let mut items = Vec::new();
                for li in node.children().filter(|c| c.is_element()) {
                    let s = self.sheet.style(li, style);
                    if s.display == Display::None {
                        continue;
                    }
                    let mut pieces = Vec::new();
                    if self.has_blocks(li, &s) {
                        self.walk(li, &s, false, &mut pieces);
                    } else {
                        self.leaf(li, &s, false, &mut pieces);
                    }
                    let blocks = group(pieces, &mut self.next_id);
                    if !blocks.is_empty() {
                        items.push(blocks);
                    }
                }
                if !items.is_empty() {
                    make(
                        BlockKind::List {
                            ordered: tag == "ol",
                            start,
                            items,
                        },
                        self,
                        out,
                    );
                }
            }
            "table" => {
                let rows: Vec<Vec<Vec<Inline>>> = node
                    .descendants()
                    .filter(|n| n.has_tag_name("tr"))
                    .map(|tr| {
                        tr.children()
                            .filter(|c| c.has_tag_name("td") || c.has_tag_name("th"))
                            .map(|cell| {
                                let s = self.sheet.style(cell, style);
                                let mut i = self.inlines(cell, &s);
                                trim(&mut i);
                                i
                            })
                            .collect()
                    })
                    .filter(|r: &Vec<Vec<Inline>>| !r.is_empty())
                    .collect();
                if !rows.is_empty() {
                    make(BlockKind::Table(rows), self, out);
                }
            }
            "img" | "svg" | "image" => {
                if let Some(image) = self.image(node, style) {
                    let para = Block {
                        id: None,
                        kind: BlockKind::Paragraph(vec![image]),
                        lang: None,
                        reference: None,
                        line: 0,
                    };
                    make(BlockKind::Figure(vec![para]), self, out);
                }
            }
            "figure" => {
                let mut pieces = Vec::new();
                self.walk(node, style, false, &mut pieces);
                let blocks = group(pieces, &mut self.next_id);
                if !blocks.is_empty() {
                    make(BlockKind::Figure(blocks), self, out);
                }
            }
            "blockquote" if !verse_here => {
                let mut pieces = Vec::new();
                self.walk(node, style, false, &mut pieces);
                let blocks = group(pieces, &mut self.next_id);
                if blocks
                    .iter()
                    .all(|b| matches!(b.kind, BlockKind::Verse(_) | BlockKind::Stanza(_)))
                {
                    // Verse set off as a quotation is just verse.
                    out.extend(blocks.into_iter().map(Piece::Block));
                } else if !blocks.is_empty() {
                    make(BlockKind::Quote(blocks), self, out);
                }
            }
            _ if self.has_blocks(node, style) => self.walk(node, style, verse_here, out),
            _ => self.leaf(node, style, verse_here, out),
        }
    }

    fn leaf(&mut self, node: Node, style: &Style, in_verse: bool, out: &mut Vec<Piece>) {
        let mut inlines = self.inlines(node, style);
        trim(&mut inlines);
        let tag = node.tag_name().name().to_ascii_lowercase();
        let empty = inlines
            .iter()
            .all(|i| matches!(i, Inline::PageBreak { .. }));
        let class = node
            .attribute("class")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if empty {
            // A spacer that marks a break in the text.
            if class.contains("break") || class.contains("space") || class.contains("orn") {
                out.push(Piece::Block(Block {
                    id: Some(self.id()),
                    kind: BlockKind::SceneBreak,
                    lang: None,
                    reference: None,
                    line: 0,
                }));
            }
            return;
        }
        out.push(Piece::Leaf(Leaf {
            inlines,
            style: style.clone(),
            tag,
            id: node.attribute("id").map(str::to_string),
            lang: lang_of(node),
            parent: node.parent().map_or(node.id(), |p| p.id()),
            in_verse,
        }));
    }

    // ---- inlines ------------------------------------------------------

    fn inlines(&mut self, node: Node, style: &Style) -> Vec<Inline> {
        let mut out = Vec::new();
        for child in node.children() {
            self.inline_node(child, style, &mut out);
        }
        out
    }

    fn inline_node(&mut self, n: Node, parent: &Style, out: &mut Vec<Inline>) {
        if n.is_text() {
            let text = n.text().unwrap_or_default();
            let text = if parent.pre {
                text.to_string()
            } else {
                collapse(text)
            };
            let text = if parent.uppercase {
                text.to_uppercase()
            } else {
                text
            };
            if !text.is_empty() {
                out.push(Inline::Text(text));
            }
            return;
        }
        if !n.is_element() {
            return;
        }
        let s = self.sheet.style(n, parent);
        if s.display == Display::None || self.skipped(n) {
            return;
        }
        let tag = n.tag_name().name().to_ascii_lowercase();
        if is_pagebreak(n) {
            let number = n
                .attribute("title")
                .or_else(|| {
                    n.attributes()
                        .find(|a| a.name() == "aria-label")
                        .map(|a| a.value())
                })
                .map(str::to_string)
                .unwrap_or_else(|| {
                    text_of(n)
                        .trim()
                        .trim_matches(|c| c == '{' || c == '}')
                        .to_string()
                });
            out.push(Inline::PageBreak {
                n: number,
                edition: None,
            });
            return;
        }
        match tag.as_str() {
            "br" => {
                out.push(Inline::LineBreak);
                return;
            }
            "img" | "image" | "svg" => {
                if let Some(cap) = self.drop_cap(n) {
                    out.push(Inline::Text(cap));
                } else if let Some(image) = self.image(n, &s) {
                    out.push(image);
                }
                return;
            }
            "a" => {
                if let Some(href) = n.attribute("href") {
                    let target = resolve(&self.path, href);
                    if let Some(label) = self.labels.get(&target) {
                        out.push(Inline::NoteRef(label.clone()));
                        return;
                    }
                }
                // A note's link back to the text has nowhere to go here.
                if is_backlink(n) {
                    return;
                }
            }
            _ => {}
        }
        let mut inner = Vec::new();
        for c in n.children() {
            self.inline_node(c, &s, &mut inner);
        }
        if inner.is_empty() {
            return;
        }
        let mut wrapped = inner;
        if s.monospace && !parent.monospace && !parent.pre {
            wrapped = vec![Inline::Span {
                kind: SpanKind::Code,
                lang: None,
                inlines: wrapped,
            }];
        }
        if s.small_caps && !parent.small_caps {
            wrapped = vec![Inline::Span {
                kind: SpanKind::SmallCaps,
                lang: None,
                inlines: wrapped,
            }];
        }
        if s.bold && !parent.bold {
            wrapped = vec![Inline::Strong(wrapped)];
        }
        if s.italic && !parent.italic {
            wrapped = vec![Inline::Emphasis(wrapped)];
        }
        if let Some(lang) = lang_of(n) {
            wrapped = vec![Inline::Span {
                kind: SpanKind::Foreign,
                lang: Some(lang),
                inlines: wrapped,
            }];
        }
        out.extend(wrapped);
    }

    /// An image, with the share of the line its CSS asks for.
    fn image(&self, n: Node, style: &Style) -> Option<Inline> {
        let src = n
            .attribute("src")
            .or_else(|| {
                n.attributes()
                    .find(|a| a.name() == "href")
                    .map(|a| a.value())
            })
            .or_else(|| {
                n.descendants()
                    .find(|d| d.has_tag_name("image"))
                    .and_then(|i| {
                        i.attributes()
                            .find(|a| a.name() == "href")
                            .map(|a| a.value())
                    })
            })?;
        let alt = n.attribute("alt").unwrap_or_default().trim();
        let width = style
            .width
            .filter(|w| w.percent > 0.0)
            .map(|w| w.percent.min(1.0));
        // In ems of the body text, whatever size the image's own text is.
        let height = style
            .height
            .filter(|h| h.em > 0.0)
            .map(|h| h.em * style.size);
        Some(Inline::Image {
            src: resolve(&self.path, src),
            alt: if alt.is_empty() {
                Vec::new()
            } else {
                vec![Inline::Text(alt.to_string())]
            },
            width,
            height,
        })
    }

    /// An image that stands for the first letter of what follows, like
    /// Crawford's decorated capitals: the letter it stands for.
    fn drop_cap(&self, n: Node) -> Option<String> {
        let alt = n.attribute("alt").unwrap_or_default().trim();
        if alt.is_empty() {
            return None;
        }
        let following: String = n.next_siblings().skip(1).map(text_of).collect::<String>();
        let next_word: String = following
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphabetic())
            .collect();
        let class = n
            .attribute("class")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let marked = ["drop", "initial", "cap"].iter().any(|k| class.contains(k));
        let upper = next_word.chars().next().is_some_and(|c| c.is_uppercase());
        if !marked && !upper {
            return None;
        }
        let last = alt
            .split_whitespace()
            .last()
            .unwrap_or(alt)
            .trim_matches(|c: char| !c.is_alphabetic());
        // "…decorated with a tree. Heed" before "EED": the letter is "H".
        if !next_word.is_empty()
            && last.chars().count() > next_word.chars().count()
            && last.to_uppercase().ends_with(&next_word.to_uppercase())
        {
            let n = last.chars().count() - next_word.chars().count();
            let letter: String = last.chars().take(n).collect();
            return Some(if upper { letter.to_uppercase() } else { letter });
        }
        if alt.chars().count() == 1 {
            return Some(alt.to_string());
        }
        // "Capital letter H …"
        let words: Vec<&str> = alt.split_whitespace().collect();
        words
            .windows(2)
            .find(|w| {
                w[0].eq_ignore_ascii_case("letter")
                    && w[1]
                        .trim_matches(|c: char| !c.is_alphabetic())
                        .chars()
                        .count()
                        == 1
            })
            .map(|w| w[1].trim_matches(|c: char| !c.is_alphabetic()).to_string())
            .or(marked.then(String::new))
    }
}

// ---- grouping ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Hint {
    Prose,
    /// A short line with no indent: verse in a run of three.
    Maybe,
    Verse,
}

fn hint(leaf: &Leaf) -> Hint {
    let len = leaf.text().chars().count();
    // A heading is never a line of verse, even inside a poem.
    if leaf.tag.len() == 2
        && leaf.tag.starts_with('h')
        && leaf.tag[1..].chars().all(|c| c.is_ascii_digit())
    {
        return Hint::Prose;
    }
    if leaf.in_verse {
        return Hint::Verse;
    }
    if len > SHORT_LINE || is_heading(leaf) {
        return Hint::Prose;
    }
    if leaf.hanging() {
        return Hint::Verse;
    }
    if leaf.style.text_indent.em.abs() < 0.1 && leaf.style.align != Align::Center && len > 0 {
        return Hint::Maybe;
    }
    Hint::Prose
}

fn is_heading(leaf: &Leaf) -> bool {
    if let Some(level) = leaf.tag.strip_prefix('h') {
        if level.len() == 1 && level.chars().all(|c| ('1'..='6').contains(&c)) {
            return true;
        }
    }
    let short = leaf.text().chars().count() <= 100;
    let s = &leaf.style;
    short
        && (s.size >= 1.15
            || (s.bold && s.align == Align::Center)
            || (s.uppercase && s.align == Align::Center))
}

fn heading_level(leaf: &Leaf) -> u16 {
    if let Some(level) = leaf
        .tag
        .strip_prefix('h')
        .and_then(|l| l.parse::<u16>().ok())
    {
        return level.clamp(1, 3);
    }
    match leaf.style.size {
        s if s >= 1.6 => 1,
        s if s >= 1.25 => 2,
        _ => 3,
    }
}

fn group(pieces: Vec<Piece>, next_id: &mut usize) -> Vec<Block> {
    let hints: Vec<Option<Hint>> = pieces
        .iter()
        .map(|p| match p {
            Piece::Leaf(l) => Some(hint(l)),
            Piece::Block(_) => None,
        })
        .collect();
    // Which leaves are verse: certain ones, and runs of three likely ones.
    let mut verse = vec![false; pieces.len()];
    let mut k = 0;
    while k < pieces.len() {
        match hints[k] {
            Some(Hint::Verse) => {
                verse[k] = true;
                k += 1;
            }
            Some(Hint::Maybe) => {
                let end = (k..pieces.len())
                    .find(|&j| !matches!(hints[j], Some(Hint::Maybe | Hint::Verse)))
                    .unwrap_or(pieces.len());
                if end - k >= 3 {
                    verse[k..end].iter_mut().for_each(|v| *v = true);
                }
                k = end.max(k + 1);
            }
            _ => k += 1,
        }
    }

    let mut out = Vec::new();
    let mut lines: Vec<Leaf> = Vec::new();
    let fresh = |n: &mut usize| {
        *n += 1;
        format!("b{n}")
    };
    for (piece, is_verse) in pieces.into_iter().zip(verse) {
        match piece {
            Piece::Leaf(leaf) if is_verse => lines.push(leaf),
            other => {
                if !lines.is_empty() {
                    out.extend(verse_blocks(std::mem::take(&mut lines), next_id));
                }
                match other {
                    Piece::Block(b) => out.push(b),
                    Piece::Leaf(leaf) => out.push(prose(leaf, fresh(next_id))),
                }
            }
        }
    }
    if !lines.is_empty() {
        out.extend(verse_blocks(lines, next_id));
    }
    // No break at the top, and never two in a row.
    let mut cleaned: Vec<Block> = Vec::with_capacity(out.len());
    for b in out {
        let is_break = matches!(b.kind, BlockKind::SceneBreak);
        let after_break = cleaned
            .last()
            .is_none_or(|p| matches!(p.kind, BlockKind::SceneBreak | BlockKind::Heading { .. }));
        if is_break && after_break {
            continue;
        }
        cleaned.push(b);
    }
    while cleaned
        .last()
        .is_some_and(|b| matches!(b.kind, BlockKind::SceneBreak))
    {
        cleaned.pop();
    }
    cleaned
}

fn prose(leaf: Leaf, fallback_id: String) -> Block {
    let id = Some(leaf.id.clone().unwrap_or(fallback_id));
    let text = leaf.text();
    let ornament = !text.is_empty()
        && text.chars().count() <= 12
        && text.chars().all(|c| !c.is_alphanumeric());
    let kind = if ornament {
        BlockKind::SceneBreak
    } else if is_heading(&leaf) {
        BlockKind::Heading {
            level: heading_level(&leaf),
            text: leaf.inlines,
        }
    } else if leaf.style.align == Align::Center {
        BlockKind::Center(vec![Block {
            id: None,
            kind: BlockKind::Paragraph(leaf.inlines),
            lang: None,
            reference: None,
            line: 0,
        }])
    } else if leaf.style.left().em >= 1.5 {
        BlockKind::Quote(vec![Block {
            id: None,
            kind: BlockKind::Paragraph(leaf.inlines),
            lang: None,
            reference: None,
            line: 0,
        }])
    } else {
        BlockKind::Paragraph(leaf.inlines)
    };
    Block {
        id,
        kind,
        lang: leaf.lang,
        reference: None,
        line: 0,
    }
}

/// Lines into verse. A line that starts with a bare number opens a stanza
/// with that number; a bare number elsewhere in a line is its line number.
/// A new parent element, or space above, starts a new group of lines.
fn verse_blocks(leaves: Vec<Leaf>, next_id: &mut usize) -> Vec<Block> {
    let base = leaves.iter().map(Leaf::start).fold(f32::MAX, f32::min);
    let mut blocks: Vec<Block> = Vec::new();
    let mut verse = Verse::default();
    let mut number: Option<String> = None;
    let mut group = 0;
    let mut parent = None;
    let mut first_id = None;
    let mut flush =
        |verse: &mut Verse, number: &mut Option<String>, first_id: &mut Option<String>| {
            if verse.lines.is_empty() {
                return;
            }
            *next_id += 1;
            let v = std::mem::take(verse);
            blocks.push(Block {
                id: Some(first_id.take().unwrap_or_else(|| format!("b{next_id}"))),
                kind: if number.is_some() {
                    BlockKind::Stanza(v)
                } else {
                    BlockKind::Verse(v)
                },
                lang: None,
                reference: number.take(),
                line: 0,
            });
        };
    for mut leaf in leaves {
        let bracketed = bracketed_number(&mut leaf.inlines);
        let lead = if bracketed.is_some() {
            None
        } else {
            leading_number(&mut leaf.inlines)
        };
        let line_no = bracketed.or_else(|| trailing_number(&mut leaf.inlines));
        if let Some(n) = lead {
            flush(&mut verse, &mut number, &mut first_id);
            number = Some(n);
            group = 0;
        } else if (parent.is_some_and(|p| p != leaf.parent) || leaf.style.margin_top.em > 0.5)
            && !verse.lines.is_empty()
        {
            group += 1;
        }
        parent = Some(leaf.parent);
        if first_id.is_none() {
            first_id = leaf.id.clone();
        }
        // The layout's indent step is 1.5 em.
        let indent = ((leaf.start() - base) / 1.5).round().clamp(0.0, 8.0) as u8;
        verse.lines.push(VerseLine {
            indent,
            group,
            inlines: leaf.inlines,
            reference: line_no,
            line: 0,
        });
    }
    flush(&mut verse, &mut number, &mut first_id);
    blocks
}

/// A bare number at the start of a line, taken out.
fn leading_number(inlines: &mut Vec<Inline>) -> Option<String> {
    let first = inlines
        .iter()
        .position(|i| !matches!(i, Inline::PageBreak { .. }))?;
    let Inline::Text(t) = &inlines[first] else {
        return None;
    };
    let trimmed = t.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    // Three digits at most: "1914 was the year" isn't stanza 1914.
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let rest = &trimmed[digits.len()..];
    // "1 Heed…" or "1" alone in its own element, then the line.
    if !(rest.is_empty() || rest.starts_with(' '))
        || (rest.is_empty() && inlines.len() == first + 1)
    {
        return None;
    }
    let rest = rest.trim_start().to_string();
    if rest.is_empty() {
        inlines.remove(first);
    } else {
        inlines[first] = Inline::Text(rest);
    }
    trim(inlines);
    Some(digits)
}

/// "[40] swords and body-armour": a line number in brackets at the start
/// of a line, as Penguin sets them, taken out.
fn bracketed_number(inlines: &mut Vec<Inline>) -> Option<String> {
    let first = inlines
        .iter()
        .position(|i| !matches!(i, Inline::PageBreak { .. }))?;
    let text = match &inlines[first] {
        Inline::Text(t) => t.clone(),
        Inline::Span { inlines: inner, .. } => plain_text(inner),
        _ => return None,
    };
    let trimmed = text.trim_start();
    let inner = trimmed.strip_prefix('[')?;
    let close = inner.find(']')?;
    let digits = &inner[..close];
    if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rest = inner[close + 1..].trim_start().to_string();
    match (&inlines[first], rest.is_empty()) {
        (_, true) => {
            inlines.remove(first);
        }
        (Inline::Text(_), false) => inlines[first] = Inline::Text(rest),
        // A span holding more than the number: leave it.
        _ => return None,
    }
    trim(inlines);
    Some(digits.to_string())
}

/// A bare number at the end of a line: its line number, taken out.
fn trailing_number(inlines: &mut Vec<Inline>) -> Option<String> {
    let last = inlines
        .iter()
        .rposition(|i| !matches!(i, Inline::PageBreak { .. }))?;
    let Inline::Text(t) = &inlines[last] else {
        return None;
    };
    let trimmed = t.trim_end();
    let digits: String = trimmed
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let rest = &trimmed[..trimmed.len() - digits.len()];
    if !(rest.is_empty() || rest.ends_with(char::is_whitespace)) || (rest.is_empty() && last == 0) {
        return None;
    }
    let rest = rest.trim_end().to_string();
    if rest.is_empty() {
        inlines.remove(last);
    } else {
        inlines[last] = Inline::Text(rest);
    }
    trim(inlines);
    Some(digits)
}

/// A note's own number at its start, and its link back to the text, are
/// furniture: the panel shows the number, and the link has nowhere to go.
fn strip_note_furniture(blocks: &mut [Block], target: Node, back: Option<&str>) {
    let label = text_of(target).trim().to_string();
    let target_is_inline =
        target.has_tag_name("a") || target.has_tag_name("span") || target.has_tag_name("sup");
    for (k, b) in blocks.iter_mut().enumerate() {
        let BlockKind::Paragraph(inlines) = &mut b.kind else {
            continue;
        };
        if k == 0 && target_is_inline && !label.is_empty() {
            strip_leading(inlines, &label);
        }
        strip_backlinks(inlines, back);
        trim(inlines);
    }
}

fn strip_leading(inlines: &mut Vec<Inline>, label: &str) {
    let text = plain_text(inlines);
    if !text.trim_start().starts_with(label) {
        return;
    }
    let mut left = label.chars().count();
    while left > 0 && !inlines.is_empty() {
        match &mut inlines[0] {
            Inline::Text(t) => {
                let t2 = t.trim_start().to_string();
                let n = t2.chars().count();
                if n <= left {
                    left -= n;
                    inlines.remove(0);
                } else {
                    *t = t2.chars().skip(left).collect();
                    left = 0;
                }
            }
            Inline::Emphasis(c)
            | Inline::Strong(c)
            | Inline::Span { inlines: c, .. }
            | Inline::Link { inlines: c, .. } => {
                let n = plain_text(c).trim().chars().count();
                if n <= left {
                    left -= n;
                    inlines.remove(0);
                } else {
                    return;
                }
            }
            _ => {
                inlines.remove(0);
            }
        }
    }
    // "1. Text" and "1 Text": the dot or space after the number goes too.
    if let Some(Inline::Text(t)) = inlines.first_mut() {
        *t = t
            .trim_start_matches(['.', ')', ':', ' ', '\u{a0}'])
            .to_string();
    }
}

fn strip_backlinks(inlines: &mut Vec<Inline>, back: Option<&str>) {
    inlines.retain(|i| match i {
        Inline::Link { target, inlines } => {
            let text = plain_text(inlines);
            let arrow = text
                .chars()
                .all(|c| matches!(c, '↩' | '↑' | '⤴' | '^' | ' ' | '\u{fe0e}'))
                && !text.trim().is_empty();
            !(arrow
                || back.is_some_and(|b| target.ends_with(b))
                || text.trim().eq_ignore_ascii_case("back"))
        }
        _ => true,
    });
}

// ---- helpers ----------------------------------------------------------

fn is_block(d: Display) -> bool {
    !matches!(d, Display::Inline | Display::None)
}

fn epub_type(n: Node) -> String {
    n.attributes()
        .filter(|a| a.name() == "type" || a.name() == "role")
        .map(|a| a.value().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_noteref(a: Node) -> bool {
    let t = epub_type(a);
    if t.contains("noteref") {
        return true;
    }
    if t.contains("backlink") || t.contains("doc-backlink") {
        return false;
    }
    let text = text_of(a);
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 4 {
        return false;
    }
    let superscript = a.ancestors().take(3).any(|n| n.has_tag_name("sup"))
        || a.children().any(|c| c.has_tag_name("sup"));
    let class = format!(
        "{} {}",
        a.attribute("class").unwrap_or_default(),
        a.parent()
            .and_then(|p| p.attribute("class"))
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    let classed = ["note", "fn", "footnote", "endnote", "ref"]
        .iter()
        .any(|k| class.contains(k));
    let id = a.attribute("id").unwrap_or_default().to_ascii_lowercase();
    let id_says = ["fn", "note"].iter().any(|k| id.contains(k));
    let in_heading = a.ancestors().any(|n| {
        let tag = n.tag_name().name();
        tag.len() == 2 && tag.starts_with('h') && tag[1..].chars().all(|c| c.is_ascii_digit())
    });
    !in_heading && (superscript || classed || id_says)
}

fn is_backlink(a: Node) -> bool {
    let t = epub_type(a);
    if t.contains("backlink") {
        return true;
    }
    let text = text_of(a);
    let text = text.trim();
    !text.is_empty()
        && text
            .chars()
            .all(|c| matches!(c, '↩' | '↑' | '⤴' | '\u{fe0e}' | '\u{fe0f}'))
}

fn is_note_body(n: Node) -> bool {
    let t = epub_type(n);
    ["footnote", "endnote", "rearnote"].iter().any(|k| t.split_whitespace().any(|w| w.ends_with(k)))
        && !t.contains("footnotes") // a whole section of them stays
        && !t.contains("endnotes")
}

fn is_pagebreak(n: Node) -> bool {
    epub_type(n).contains("pagebreak")
}

fn names_verse(n: Node) -> bool {
    let words = format!(
        "{} {}",
        n.attribute("class").unwrap_or_default(),
        epub_type(n)
    )
    .to_ascii_lowercase();
    ["verse", "poem", "poetry", "stanza", "lg"]
        .iter()
        .any(|k| words.split(|c: char| !c.is_alphanumeric()).any(|w| w == *k))
}

fn lang_of(n: Node) -> Option<String> {
    n.attributes()
        .find(|a| a.name() == "lang")
        .map(|a| a.value().to_string())
}

fn text_of(n: Node) -> String {
    n.descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect()
}

fn pre_text(n: Node) -> String {
    let mut out = String::new();
    for d in n.descendants() {
        if d.has_tag_name("br") {
            out.push('\n');
        } else if let Some(t) = d.text().filter(|_| d.is_text()) {
            out.push_str(t);
        }
    }
    out.trim_matches('\n').to_string()
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Spaces off both ends of a run of inlines, into nested ones; and the
/// spaces between two texts merged.
fn trim(inlines: &mut Vec<Inline>) {
    fn first_text(list: &mut [Inline]) -> Option<&mut String> {
        for i in list.iter_mut() {
            match i {
                Inline::Text(t) => return Some(t),
                Inline::Emphasis(c)
                | Inline::Strong(c)
                | Inline::Span { inlines: c, .. }
                | Inline::Link { inlines: c, .. } => return first_text(c),
                Inline::PageBreak { .. } => continue,
                _ => return None,
            }
        }
        None
    }
    fn last_text(list: &mut [Inline]) -> Option<&mut String> {
        for i in list.iter_mut().rev() {
            match i {
                Inline::Text(t) => return Some(t),
                Inline::Emphasis(c)
                | Inline::Strong(c)
                | Inline::Span { inlines: c, .. }
                | Inline::Link { inlines: c, .. } => return last_text(c),
                Inline::PageBreak { .. } => continue,
                _ => return None,
            }
        }
        None
    }
    // Line breaks at either end mean nothing.
    while matches!(inlines.first(), Some(Inline::LineBreak)) {
        inlines.remove(0);
    }
    while matches!(inlines.last(), Some(Inline::LineBreak)) {
        inlines.pop();
    }
    if let Some(t) = first_text(inlines) {
        *t = t.trim_start().to_string();
    }
    if let Some(t) = last_text(inlines) {
        *t = t.trim_end().to_string();
    }
    inlines.retain(|i| !matches!(i, Inline::Text(t) if t.is_empty()));
}

#[cfg(test)]
mod tests;
