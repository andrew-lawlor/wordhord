//! A chapter's blocks set as lines, then the lines set on pages.
//!
//! Prose is hyphenated and broken by Knuth–Plass. Verse is set line by
//! line, as written; a line too wide for the page turns over, at its
//! caesura if it can, under a hanging indent. Stanza numbers sit in the
//! left margin; counted verse (Dante's lines) shows every fifth line
//! number in the right.
//!
//! A note opens in a panel over the foot of the page when its marker is
//! tapped, so notes never take room from the page. Commentary on a passage
//! opens the same way, from a mark beside the passage. Both are also
//! gathered at the end of the chapter, to read straight through.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use hyphenation::{Hyphenator, Language, Load, Standard};
use wordhord_book::model::{plain_text, walk, Block, BlockKind, Inline, SpanKind, VerseLine};

use crate::fonts::{Face, Fonts, Glyph};
use crate::linebreak::{self, Item, INFINITY};
use crate::page::{Link, Page, Panel, Run, Target, Word};
use crate::profile::Profile;

/// Glyphs in one font, placed within a line.
#[derive(Debug, Clone)]
pub struct Frag {
    pub font: usize,
    pub size: f32,
    pub x: f32,
    /// Up from the baseline is negative (superscripts).
    pub dy: f32,
    pub glyphs: Vec<Glyph>,
}

/// One line of text, relative to the left edge of the text block.
#[derive(Debug, Clone, Default)]
pub struct Line {
    pub x: f32,
    pub frags: Vec<Frag>,
    /// Words: text, start and end within the line.
    pub words: Vec<(String, f32, f32)>,
    /// Note markers: the note's ID, start and end within the line.
    pub notes: Vec<(String, f32, f32)>,
    /// Commentary marks in the margin: the commentary's ID, and the box
    /// to tap, relative to the text block.
    pub margin_links: Vec<(String, f32, f32)>,
    /// The canonical reference of the line, or of its block.
    pub reference: Option<String>,
    /// The first line of a note in the chapter's notes: its first words
    /// (the label) link back to the marker. The note's ID, and how many
    /// words.
    pub back: Option<(String, usize)>,
    /// Numbers in the margins, placed relative to the text block.
    pub marks: Vec<Frag>,
    /// The type size, for the line's ascent and descent.
    pub size: f32,
    /// Baseline to baseline.
    pub height: f32,
    pub block: String,
}

/// Lines that are set together: a paragraph, a stanza, a heading.
#[derive(Debug, Clone, Default)]
pub struct Unit {
    pub lines: Vec<Line>,
    pub space_before: f32,
    /// May break across pages.
    pub split: bool,
    /// Keep on the page with what follows (a heading).
    pub keep_with_next: bool,
}

#[derive(Clone, Copy)]
struct Style {
    face: Face,
    size: f32,
    smcp: bool,
}

#[derive(Clone)]
enum Atom {
    Text(String, Style),
    Space,
    Caesura,
    Note(String),
}

/// What each box or penalty draws.
enum Content {
    None,
    Word { frags: Vec<Frag>, text: String },
    Note { frags: Vec<Frag>, label: String },
    Hyphen(Vec<Frag>),
    Caesura,
}

pub struct Layout<'a> {
    pub fonts: &'a Fonts,
    pub profile: &'a Profile,
    hyphenator: Option<Standard>,
    /// The body size, in pixels.
    pub body: f32,
    /// The width of the text block, in pixels.
    pub measure: f32,
    /// Commentary waiting for its passage.
    anchors: RefCell<Anchors>,
}

/// Commentary by the reference or `#id` it's on, and which have a mark.
#[derive(Default)]
struct Anchors {
    by: HashMap<String, Vec<String>>,
    placed: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossKind {
    /// A footnote, opened from its marker.
    Note,
    /// A note on a passage, opened from a mark beside it.
    Commentary,
}

/// A footnote or a commentary: what opens in the panel.
#[derive(Debug, Clone)]
pub struct Gloss<'b> {
    /// `note-3`, or the commentary's block ID.
    pub id: String,
    pub kind: GlossKind,
    /// The note's label, or the reference the commentary is on.
    pub label: String,
    /// What a reader sees above a commentary: `Stanza 5`, or the passage's
    /// first words.
    pub title: String,
    pub blocks: &'b [Block],
}

/// A chapter's footnotes and commentary, in reading order.
pub fn glosses(blocks: &[Block]) -> Vec<Gloss<'_>> {
    let mut all = Vec::new();
    walk(blocks, &mut all);
    let title = |on: &str| -> String {
        if let Some(id) = on.strip_prefix('#') {
            let block = all.iter().find(|b| b.id.as_deref() == Some(id));
            let text = block.map(|b| match &b.kind {
                BlockKind::Paragraph(inlines) | BlockKind::Heading { text: inlines, .. } => {
                    plain_text(inlines)
                }
                BlockKind::Verse(v) | BlockKind::Stanza(v) => v
                    .lines
                    .first()
                    .map_or(String::new(), |l| plain_text(&l.inlines)),
                _ => String::new(),
            });
            return match text {
                Some(t) if !t.trim().is_empty() => {
                    let words: Vec<&str> = t.split_whitespace().collect();
                    let quoted = words[..words.len().min(6)].join(" ");
                    let more = if words.len() > 6 { "…" } else { "" };
                    format!("On \u{201c}{quoted}{more}\u{201d}")
                }
                _ => "On the passage".to_string(),
            };
        }
        // A stanza, or a range of them: `Stanzas 5–6`.
        let (start, end) = on.split_once('-').map_or((on, None), |(a, b)| (a, Some(b)));
        let stanza = all.iter().any(|b| {
            matches!(b.kind, BlockKind::Stanza(_)) && b.reference.as_deref() == Some(start)
        });
        let number = start.rsplit('.').next().unwrap_or(start);
        match (stanza, end) {
            (true, None) => format!("Stanza {number}"),
            (true, Some(end)) => format!("Stanzas {number}\u{2013}{end}"),
            _ => format!("On {on}"),
        }
    };
    all.iter()
        .enumerate()
        .filter_map(|(k, b)| match &b.kind {
            BlockKind::Note { label, blocks } => Some(Gloss {
                id: format!("note-{label}"),
                kind: GlossKind::Note,
                label: label.clone(),
                title: label.clone(),
                blocks,
            }),
            BlockKind::Commentary { on, blocks } => Some(Gloss {
                id: b.id.clone().unwrap_or_else(|| format!("commentary-{k}")),
                kind: GlossKind::Commentary,
                label: on.clone(),
                title: title(on),
                blocks,
            }),
            _ => None,
        })
        .collect()
}

/// The keys a commentary's `on` can match: itself, and the start of a
/// range (`vsp.5` for `vsp.5-6`).
fn anchor_keys(on: &str) -> Vec<String> {
    let mut keys = vec![on.to_string()];
    if !on.starts_with('#') {
        if let Some((start, _)) = on.split_once('-') {
            keys.push(start.to_string());
        }
    }
    keys
}

impl<'a> Layout<'a> {
    pub fn new(fonts: &'a Fonts, profile: &'a Profile, lang: &str) -> Layout<'a> {
        let hyphenator = (lang == "eng")
            .then(|| Standard::from_embedded(Language::EnglishUS).ok())
            .flatten();
        let measure = profile.width as f32
            - profile.mm(profile.margins.left)
            - profile.mm(profile.margins.right);
        Layout {
            fonts,
            profile,
            hyphenator,
            body: profile.pt(profile.body_size),
            measure,
            anchors: RefCell::default(),
        }
    }

    fn leading(&self, size: f32) -> f32 {
        size * self.profile.leading
    }

    /// A chapter's blocks, then its commentary and its notes.
    pub fn chapter(&self, blocks: &[Block]) -> Vec<Unit> {
        let glosses = glosses(blocks);
        {
            let mut anchors = self.anchors.borrow_mut();
            *anchors = Anchors::default();
            for g in glosses.iter().filter(|g| g.kind == GlossKind::Commentary) {
                for key in anchor_keys(&g.label) {
                    anchors.by.entry(key).or_default().push(g.id.clone());
                }
            }
        }
        let mut units = Vec::new();
        let mut after_heading = true;
        for b in blocks {
            if matches!(
                b.kind,
                BlockKind::Note { .. } | BlockKind::Commentary { .. }
            ) {
                continue;
            }
            self.block(b, 0.0, self.measure, self.body, after_heading, &mut units);
            after_heading = matches!(b.kind, BlockKind::Heading { .. });
        }
        for (kind, title) in [
            (GlossKind::Commentary, "Commentary"),
            (GlossKind::Note, "Notes"),
        ] {
            let set: Vec<&Gloss> = glosses.iter().filter(|g| g.kind == kind).collect();
            if set.is_empty() {
                continue;
            }
            let heading = vec![Inline::Text(title.into())];
            units.push(self.heading(&heading, 2, &title.to_lowercase()));
            for g in set {
                units.append(&mut self.gloss(g, self.measure, true));
            }
        }
        units
    }

    /// A note or a commentary, `measure` wide. A note starts with its label
    /// in bold; a commentary with the passage it's on, in italics. With
    /// `back`, the label links back to the marker.
    pub fn gloss(&self, g: &Gloss, measure: f32, back: bool) -> Vec<Unit> {
        let size = self.body * 0.85;
        let mut units = Vec::new();
        let mut rest = g.blocks;
        let label_words;
        match (g.kind, g.blocks.first().map(|b| &b.kind)) {
            (GlossKind::Note, Some(BlockKind::Paragraph(inlines))) => {
                let mut all = vec![
                    Inline::Strong(vec![Inline::Text(format!("{}.", g.label))]),
                    Inline::Text(" ".into()),
                ];
                all.extend(inlines.iter().cloned());
                let mut u = self.paragraph(&all, 0.0, measure, size, false, &g.id);
                u.space_before = self.leading(size) * 0.4;
                units.push(u);
                rest = &g.blocks[1..];
                label_words = 1;
            }
            (GlossKind::Note, _) => {
                let label = vec![Inline::Strong(vec![Inline::Text(format!("{}.", g.label))])];
                let mut u = self.paragraph(&label, 0.0, measure, size, false, &g.id);
                u.space_before = self.leading(size) * 0.4;
                u.keep_with_next = true;
                units.push(u);
                label_words = 1;
            }
            (GlossKind::Commentary, _) => {
                let text = g.title.clone();
                label_words = text.split_whitespace().count();
                let label = vec![Inline::Emphasis(vec![Inline::Text(text)])];
                let mut u = self.paragraph(&label, 0.0, measure, size, false, &g.id);
                u.space_before = self.leading(size) * 0.6;
                u.keep_with_next = true;
                units.push(u);
            }
        }
        if back {
            if let Some(line) = units.first_mut().and_then(|u| u.lines.first_mut()) {
                line.back = Some((g.id.clone(), label_words));
            }
        }
        for (k, b) in rest.iter().enumerate() {
            // A paragraph after a paragraph is indented.
            let after = match k {
                0 => g.blocks.len() > rest.len(),
                _ => matches!(rest[k - 1].kind, BlockKind::Paragraph(_)),
            };
            let after = after && matches!(b.kind, BlockKind::Paragraph(_));
            self.block(b, 0.0, measure, size, !after, &mut units);
        }
        units
    }

    /// Where a note opens: the lower 40% of the screen.
    pub fn panel(&self) -> Panel {
        let p = self.profile;
        let y = (p.height as f32 * 0.6).round();
        Panel {
            x: 0.0,
            y,
            width: p.width as f32,
            height: p.height as f32 - y,
        }
    }

    /// A note or commentary set on panel pages, drawn from the panel's
    /// corner.
    pub fn panel_pages(&self, g: &Gloss) -> Vec<Page> {
        let p = self.profile;
        let panel = self.panel();
        let units = self.gloss(g, self.measure, false);
        let area = Area {
            top: p.mm(5.0),
            bottom: panel.height - p.mm(p.margins.bottom),
            left: p.mm(p.margins.left),
        };
        self.flow(&units, area)
    }

    fn block(
        &self,
        b: &Block,
        left: f32,
        measure: f32,
        size: f32,
        after_heading: bool,
        units: &mut Vec<Unit>,
    ) {
        let id = b.id.clone().unwrap_or_default();
        let first = units.len();
        self.block_kind(b, &id, left, measure, size, after_heading, units);
        for u in &mut units[first..] {
            for l in &mut u.lines {
                if l.reference.is_none() {
                    l.reference = b.reference.clone();
                }
            }
        }
        // Commentary on this block goes beside its first line; on a line of
        // counted verse, beside that line.
        let keys = b
            .reference
            .iter()
            .cloned()
            .chain(b.id.iter().map(|i| format!("#{i}")));
        let ids = self.claim(keys);
        if let Some(line) = units[first..]
            .iter_mut()
            .flat_map(|u| u.lines.iter_mut())
            .find(|l| !l.frags.is_empty())
        {
            self.commentary_mark(line, &ids, size);
        }
        for u in &mut units[first..] {
            for l in &mut u.lines {
                if let Some(r) = l.reference.clone() {
                    let ids = self.claim(std::iter::once(r));
                    self.commentary_mark(l, &ids, size);
                }
            }
        }
    }

    /// The commentary waiting on any of `keys` that has no mark yet.
    fn claim(&self, keys: impl Iterator<Item = String>) -> Vec<String> {
        let mut anchors = self.anchors.borrow_mut();
        let mut ids = Vec::new();
        for key in keys {
            let Some(waiting) = anchors.by.get(&key).cloned() else {
                continue;
            };
            for id in waiting {
                if anchors.placed.insert(id.clone()) {
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// A small ring in the margin, left of any stanza number, for each
    /// commentary. Tapping it, or the number, opens the commentary.
    fn commentary_mark(&self, line: &mut Line, ids: &[String], size: f32) {
        let style = Style {
            face: Face::Regular,
            size: size * 0.8,
            smcp: false,
        };
        let number = line
            .marks
            .iter()
            .filter(|f| f.x < line.x)
            .map(|f| (f.x, f.x + frag_width(f)))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
        let mut right = number.map_or(line.x, |(x0, _)| x0);
        for id in ids {
            let (frags, w) = self.frags("\u{25e6}", style, 0.0);
            let x = right - size * 0.4 - w;
            for f in frags {
                line.marks.push(Frag { x: x + f.x, ..f });
            }
            let end = number.map_or(x + w, |(_, x1)| x1);
            line.margin_links.push((id.clone(), x, end));
            right = x;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn block_kind(
        &self,
        b: &Block,
        id: &str,
        left: f32,
        measure: f32,
        size: f32,
        after_heading: bool,
        units: &mut Vec<Unit>,
    ) {
        let id = id.to_string();
        match &b.kind {
            BlockKind::Heading { level, text } => units.push(self.heading(text, *level, &id)),
            BlockKind::Paragraph(inlines) => {
                let mut u = self.paragraph(inlines, left, measure, size, !after_heading, &id);
                // A little air under a heading.
                let heading = after_heading && units.last().is_some_and(|u| u.keep_with_next);
                u.space_before = if heading {
                    self.leading(size) * 0.4
                } else {
                    0.0
                };
                units.push(u);
            }
            BlockKind::Verse(v) | BlockKind::Stanza(v) => {
                let stanza = matches!(b.kind, BlockKind::Stanza(_));
                let number = stanza
                    .then(|| {
                        b.reference
                            .as_ref()
                            .and_then(|r| r.rsplit('.').next().map(String::from))
                    })
                    .flatten();
                units.extend(self.verse(&v.lines, left, measure, size, number.as_deref(), &id));
                for s in &v.stanzas {
                    self.block(s, left, measure, size, false, units);
                }
            }
            BlockKind::Quote(blocks) | BlockKind::Epigraph(blocks) | BlockKind::Letter(blocks) => {
                let indent = 2.0 * size;
                for (k, c) in blocks.iter().enumerate() {
                    let before = units.len();
                    self.block(c, left + indent, measure - 2.0 * indent, size, true, units);
                    if k == 0 {
                        if let Some(u) = units.get_mut(before) {
                            u.space_before = self.leading(size) * 0.5;
                        }
                    }
                }
            }
            BlockKind::Center(blocks) | BlockKind::Figure(blocks) => {
                for c in blocks {
                    self.block(c, left, measure, size, true, units);
                }
            }
            BlockKind::Speech { speaker, blocks } => {
                if let Some(s) = speaker {
                    let style = Style {
                        face: Face::Regular,
                        size,
                        smcp: true,
                    };
                    let atoms = vec![Atom::Text(s.to_lowercase(), style)];
                    let mut u = Unit {
                        split: false,
                        keep_with_next: true,
                        space_before: self.leading(size) * 0.5,
                        ..Default::default()
                    };
                    u.lines = self.ragged(&atoms, left, measure, size, false, &id);
                    units.push(u);
                }
                for c in blocks {
                    self.block(c, left + size, measure - size, size, true, units);
                }
            }
            // Gathered at the end of the chapter, and opened in the panel.
            BlockKind::Commentary { .. } => {}
            BlockKind::Table(rows) => units.push(self.table(rows, left, measure, size, &id)),
            BlockKind::SceneBreak => {
                let atoms = vec![Atom::Text(
                    "*   *   *".into(),
                    Style {
                        face: Face::Regular,
                        size,
                        smcp: false,
                    },
                )];
                let mut u = Unit {
                    space_before: self.leading(size) * 0.5,
                    ..Default::default()
                };
                u.lines = self.ragged(&atoms, left, measure, size, true, &id);
                units.push(u);
            }
            BlockKind::Note { .. } => {}
        }
    }

    pub fn heading(&self, text: &[Inline], level: u16, id: &str) -> Unit {
        let size = self.body
            * match level {
                1 => 1.6,
                2 => 1.25,
                _ => 1.1,
            };
        let face = if level >= 3 {
            Face::Italic
        } else {
            Face::Regular
        };
        let atoms = self.atoms(
            text,
            Style {
                face,
                size,
                smcp: false,
            },
        );
        let lines = self.ragged(&atoms, 0.0, self.measure, size, true, id);
        Unit {
            lines,
            space_before: self.leading(self.body) * if level == 1 { 2.0 } else { 1.0 },
            split: false,
            keep_with_next: true,
        }
    }

    // ---- inline text to atoms -------------------------------------

    fn atoms(&self, inlines: &[Inline], style: Style) -> Vec<Atom> {
        let mut out = Vec::new();
        self.push_atoms(inlines, style, &mut out);
        out
    }

    fn push_atoms(&self, inlines: &[Inline], style: Style, out: &mut Vec<Atom>) {
        for i in inlines {
            match i {
                Inline::Text(t) => {
                    let mut word = String::new();
                    for c in t.chars() {
                        if c.is_whitespace() && c != '\u{a0}' {
                            if !word.is_empty() {
                                out.push(Atom::Text(std::mem::take(&mut word), style));
                            }
                            if !matches!(out.last(), Some(Atom::Space) | None) {
                                out.push(Atom::Space);
                            }
                        } else {
                            word.push(c);
                        }
                    }
                    if !word.is_empty() {
                        out.push(Atom::Text(word, style));
                    }
                }
                Inline::Emphasis(c) => self.push_atoms(
                    c,
                    Style {
                        face: style.face.italic(),
                        ..style
                    },
                    out,
                ),
                Inline::Strong(c) => self.push_atoms(
                    c,
                    Style {
                        face: style.face.bold(),
                        ..style
                    },
                    out,
                ),
                Inline::Span { kind, inlines, .. } => {
                    let s = match kind {
                        SpanKind::SmallCaps => Style {
                            smcp: true,
                            ..style
                        },
                        // Emendations in italics, as editions print them.
                        SpanKind::Corr => Style {
                            face: style.face.italic(),
                            ..style
                        },
                        _ => style,
                    };
                    self.push_atoms(inlines, s, out);
                }
                Inline::Link { inlines, .. } => self.push_atoms(inlines, style, out),
                Inline::Image { .. } => {}
                Inline::NoteRef(label) => out.push(Atom::Note(label.clone())),
                Inline::Caesura => out.push(Atom::Caesura),
                Inline::LineBreak | Inline::SoftBreak => {
                    if !matches!(out.last(), Some(Atom::Space) | None) {
                        out.push(Atom::Space);
                    }
                }
                Inline::PageBreak { .. } | Inline::SectionStart { .. } => {}
            }
        }
    }

    // ---- atoms to items --------------------------------------------

    fn frags(&self, text: &str, style: Style, dy: f32) -> (Vec<Frag>, f32) {
        let features: &[&str] = if style.smcp { &["smcp"] } else { &[] };
        let mut x = 0.0;
        let mut frags = Vec::new();
        for s in self.fonts.shape(text, style.face, style.size, features) {
            frags.push(Frag {
                font: s.font,
                size: s.size,
                x,
                dy,
                glyphs: s.glyphs,
            });
            x += s.width;
        }
        (frags, x)
    }

    /// A note marker: lining figures, so every number sits at one height.
    fn marker(&self, text: &str, style: Style, dy: f32) -> (Vec<Frag>, f32) {
        let mut x = 0.0;
        let mut frags = Vec::new();
        for s in self.fonts.shape(text, style.face, style.size, &["lnum"]) {
            frags.push(Frag {
                font: s.font,
                size: s.size,
                x,
                dy,
                glyphs: s.glyphs,
            });
            x += s.width;
        }
        (frags, x)
    }

    /// Items for a run of atoms: words as boxes (with hyphenation points
    /// and breaks after dashes), spaces as glue, caesuras as fixed gaps.
    fn items(&self, atoms: &[Atom], size: f32, hyphenate: bool) -> (Vec<Item>, Vec<Content>) {
        let space = self.fonts.space(size);
        let mut items = Vec::new();
        let mut contents = Vec::new();
        let mut word: Vec<&Atom> = Vec::new();
        let flush = |word: &mut Vec<&Atom>, items: &mut Vec<Item>, contents: &mut Vec<Content>| {
            if word.is_empty() {
                return;
            }
            let single = word.len() == 1;
            for atom in word.drain(..) {
                match atom {
                    Atom::Text(t, style) => {
                        let pieces = if single && hyphenate {
                            self.syllables(t)
                        } else {
                            vec![t.clone()]
                        };
                        for (k, piece) in pieces.iter().enumerate() {
                            if k > 0 {
                                let (hyphen, w) = self.frags("-", *style, 0.0);
                                items.push(Item::Penalty {
                                    width: w,
                                    cost: 50.0,
                                    flagged: true,
                                });
                                contents.push(Content::Hyphen(hyphen));
                            }
                            // A dash may end a line.
                            let parts: Vec<&str> = piece.split_inclusive('—').collect();
                            for (j, part) in parts.iter().enumerate() {
                                if j > 0 {
                                    items.push(Item::Penalty {
                                        width: 0.0,
                                        cost: 50.0,
                                        flagged: false,
                                    });
                                    contents.push(Content::None);
                                }
                                let (frags, width) = self.frags(part, *style, 0.0);
                                items.push(Item::Box { width });
                                contents.push(Content::Word {
                                    frags,
                                    text: part.to_string(),
                                });
                            }
                        }
                    }
                    Atom::Note(label) => {
                        let style = Style {
                            face: Face::Regular,
                            size: size * 0.62,
                            smcp: false,
                        };
                        let (frags, width) = self.marker(label, style, -size * 0.36);
                        items.push(Item::Box { width });
                        contents.push(Content::Note {
                            frags,
                            label: format!("note-{label}"),
                        });
                    }
                    _ => {}
                }
            }
        };
        // A note marker belongs to the word before it, even if the source
        // has a space between them.
        let atoms: Vec<&Atom> = atoms
            .iter()
            .enumerate()
            .filter(|(k, a)| {
                !(matches!(a, Atom::Space) && matches!(atoms.get(k + 1), Some(Atom::Note(_))))
            })
            .map(|(_, a)| a)
            .collect();
        for a in atoms {
            match a {
                Atom::Space => {
                    flush(&mut word, &mut items, &mut contents);
                    if !items.is_empty() {
                        items.push(Item::Glue {
                            width: space,
                            stretch: space * 0.5,
                            shrink: space * 0.33,
                        });
                        contents.push(Content::None);
                    }
                }
                Atom::Caesura => {
                    flush(&mut word, &mut items, &mut contents);
                    // A caesura replaces the spaces around it.
                    if matches!(items.last(), Some(Item::Glue { .. })) {
                        items.pop();
                        contents.pop();
                    }
                    items.push(Item::Glue {
                        width: size * 1.4,
                        stretch: 0.0,
                        shrink: 0.0,
                    });
                    contents.push(Content::Caesura);
                }
                other => word.push(other),
            }
        }
        flush(&mut word, &mut items, &mut contents);
        // Drop glue after a caesura, and trailing glue.
        let mut k = 1;
        while k < items.len() {
            if matches!(contents[k - 1], Content::Caesura)
                && matches!(items[k], Item::Glue { .. })
                && matches!(contents[k], Content::None)
            {
                items.remove(k);
                contents.remove(k);
            } else {
                k += 1;
            }
        }
        while matches!(items.last(), Some(Item::Glue { .. })) {
            items.pop();
            contents.pop();
        }
        (items, contents)
    }

    /// An English word's hyphenation points, as pieces. Only plain
    /// lowercase-able ASCII words of five letters or more.
    fn syllables(&self, word: &str) -> Vec<String> {
        let Some(h) = &self.hyphenator else {
            return vec![word.to_string()];
        };
        let core: &str = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
        if core.len() < 5 || !core.chars().all(|c| c.is_ascii_alphabetic()) {
            return vec![word.to_string()];
        }
        let offset = word.find(core).unwrap_or(0);
        let lower = core.to_ascii_lowercase();
        let breaks = h.hyphenate(&lower).breaks;
        let mut pieces = Vec::new();
        let mut last = 0;
        for b in breaks {
            pieces.push(word[last..offset + b].to_string());
            last = offset + b;
        }
        pieces.push(word[last..].to_string());
        pieces
    }

    // ---- lines -----------------------------------------------------

    /// Lines from items and their breaks. Justified lines stretch their
    /// glue by the breaker's ratio; ragged ones keep natural spaces.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        &self,
        items: &[Item],
        contents: &[Content],
        breaks: &[usize],
        ratios: &[f32],
        justify: bool,
        size: f32,
        block: &str,
    ) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut start = 0;
        for (n, &b) in breaks.iter().enumerate() {
            let ratio = if justify && n + 1 < breaks.len() {
                ratios[n]
            } else {
                0.0
            };
            while start < b && matches!(items[start], Item::Glue { .. } | Item::Penalty { .. }) {
                start += 1;
            }
            let mut line = Line {
                size,
                height: self.leading(size),
                block: block.to_string(),
                ..Default::default()
            };
            let mut x = 0.0;
            for k in start..b {
                match (&items[k], &contents[k]) {
                    (Item::Box { width }, Content::Word { frags, text }) => {
                        for f in frags {
                            line.frags.push(Frag {
                                x: x + f.x,
                                ..f.clone()
                            });
                        }
                        if !text.is_empty() {
                            line.words.push((text.clone(), x, x + width));
                        }
                        x += width;
                    }
                    // A paragraph's first-line indent.
                    (Item::Box { width }, Content::None) => x += width,
                    (Item::Box { width }, Content::Note { frags, label }) => {
                        for f in frags {
                            line.frags.push(Frag {
                                x: x + f.x,
                                ..f.clone()
                            });
                        }
                        line.notes.push((label.clone(), x, x + width));
                        x += width;
                    }
                    (
                        Item::Glue {
                            width,
                            stretch,
                            shrink,
                        },
                        _,
                    ) => {
                        x += width
                            + if ratio >= 0.0 {
                                ratio * stretch
                            } else {
                                ratio * shrink
                            };
                    }
                    _ => {}
                }
            }
            if let (Item::Penalty { flagged: true, .. }, Content::Hyphen(frags)) =
                (&items[b], &contents[b])
            {
                for f in frags {
                    line.frags.push(Frag {
                        x: x + f.x,
                        ..f.clone()
                    });
                }
            }
            lines.push(line);
            start = b + 1;
        }
        lines
    }

    fn paragraph(
        &self,
        inlines: &[Inline],
        left: f32,
        measure: f32,
        size: f32,
        indent: bool,
        block: &str,
    ) -> Unit {
        let atoms = self.atoms(
            inlines,
            Style {
                face: Face::Regular,
                size,
                smcp: false,
            },
        );
        let (mut items, mut contents) = self.items(&atoms, size, true);
        let first = if indent { size * 1.5 } else { 0.0 };
        if first > 0.0 {
            items.insert(0, Item::Box { width: first });
            contents.insert(0, Content::None);
        }
        linebreak::finish(&mut items);
        contents.push(Content::None);
        contents.push(Content::None);
        let (at, ratios) = match linebreak::break_lines(&items, &|_| measure) {
            Some(b) => (b.at, b.ratios),
            None => greedy(&items, measure),
        };
        let mut lines = self.assemble(&items, &contents, &at, &ratios, true, size, block);
        for l in &mut lines {
            l.x = left;
        }
        Unit {
            lines,
            split: true,
            ..Default::default()
        }
    }

    /// Unjustified lines, broken where they must be; centred if asked.
    fn ragged(
        &self,
        atoms: &[Atom],
        left: f32,
        measure: f32,
        size: f32,
        center: bool,
        block: &str,
    ) -> Vec<Line> {
        let (mut items, mut contents) = self.items(atoms, size, false);
        linebreak::finish(&mut items);
        contents.push(Content::None);
        contents.push(Content::None);
        let (at, ratios) = greedy(&items, measure);
        let mut lines = self.assemble(&items, &contents, &at, &ratios, false, size, block);
        for l in &mut lines {
            let width = l
                .frags
                .iter()
                .map(|f| f.x + frag_width(f))
                .fold(0.0, f32::max);
            l.x = left
                + if center {
                    ((measure - width) / 2.0).max(0.0)
                } else {
                    0.0
                };
        }
        lines
    }

    /// Verse: each line as written. Too wide, it turns over (at its
    /// caesura if that's far enough along) under a hanging indent.
    fn verse(
        &self,
        lines: &[VerseLine],
        left: f32,
        measure: f32,
        size: f32,
        number: Option<&str>,
        block: &str,
    ) -> Vec<Unit> {
        let verse_left = left + size * 2.0;
        let step = size * 1.5;
        let mut unit = Unit {
            space_before: self.leading(size) * 0.6,
            split: lines.len() > 12,
            ..Default::default()
        };
        let mut group = lines.first().map_or(0, |l| l.group);
        for (k, vl) in lines.iter().enumerate() {
            if vl.group != group {
                group = vl.group;
                unit.lines.push(Line {
                    size,
                    height: self.leading(size) * 0.6,
                    block: block.to_string(),
                    ..Default::default()
                });
            }
            let atoms = self.atoms(
                &vl.inlines,
                Style {
                    face: Face::Regular,
                    size,
                    smcp: false,
                },
            );
            let x = verse_left + vl.indent as f32 * step;
            let mut set = self.turn_over(&atoms, x, measure - (x - left), size, block);
            for l in &mut set {
                l.reference = vl.reference.clone();
            }
            if k == 0 {
                if let (Some(n), Some(first)) = (number, set.first_mut()) {
                    let style = Style {
                        face: Face::Regular,
                        size: size * 0.8,
                        smcp: false,
                    };
                    let (frags, w) = self.frags(n, style, 0.0);
                    for f in frags {
                        first.marks.push(Frag {
                            x: verse_left - size * 0.8 - w + f.x,
                            ..f
                        });
                    }
                }
            }
            // Every fifth line of counted verse, in the right margin.
            if let Some(n) = vl
                .reference
                .as_deref()
                .and_then(|r| r.rsplit('.').next())
                .and_then(|n| n.parse::<u32>().ok())
            {
                if number.is_none() && n % 5 == 0 {
                    if let Some(first) = set.first_mut() {
                        let style = Style {
                            face: Face::Regular,
                            size: size * 0.7,
                            smcp: false,
                        };
                        let (frags, w) = self.frags(&n.to_string(), style, 0.0);
                        for f in frags {
                            first.marks.push(Frag {
                                x: left + measure - w + f.x,
                                ..f
                            });
                        }
                    }
                }
            }
            unit.lines.append(&mut set);
        }
        vec![unit]
    }

    fn turn_over(&self, atoms: &[Atom], x: f32, measure: f32, size: f32, block: &str) -> Vec<Line> {
        let hang = size * 2.5;
        let (mut items, mut contents) = self.items(atoms, size, false);
        let natural: f32 = items
            .iter()
            .map(|i| match i {
                Item::Box { width } | Item::Glue { width, .. } => *width,
                _ => 0.0,
            })
            .sum();
        let mut breaks = Vec::new();
        if natural > measure {
            // Prefer the last caesura past a third of the line that fits.
            let mut w = 0.0;
            let mut caesura = None;
            let mut space = None;
            for (k, item) in items.iter().enumerate() {
                if let Item::Glue { .. } = item {
                    if w <= measure {
                        if matches!(contents[k], Content::Caesura) && w > measure / 3.0 {
                            caesura = Some(k);
                        } else {
                            space = Some(k);
                        }
                    }
                }
                if let Item::Box { width } | Item::Glue { width, .. } = item {
                    w += width;
                }
            }
            if let Some(b) = caesura.or(space) {
                breaks.push(b);
            }
        }
        linebreak::finish(&mut items);
        contents.push(Content::None);
        contents.push(Content::None);
        breaks.push(items.len() - 1);
        let ratios = vec![0.0; breaks.len()];
        let mut lines = self.assemble(&items, &contents, &breaks, &ratios, false, size, block);
        for (k, l) in lines.iter_mut().enumerate() {
            l.x = if k == 0 { x } else { x + hang };
        }
        // A turned-over line may still be too wide: turn it again.
        lines
    }

    fn table(
        &self,
        rows: &[Vec<Vec<Inline>>],
        left: f32,
        measure: f32,
        size: f32,
        block: &str,
    ) -> Unit {
        let cells: Vec<Vec<Vec<Line>>> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| {
                        let atoms = self.atoms(
                            c,
                            Style {
                                face: Face::Regular,
                                size,
                                smcp: false,
                            },
                        );
                        self.ragged(&atoms, 0.0, measure, size, false, block)
                    })
                    .collect()
            })
            .collect();
        let columns = cells.iter().map(|r| r.len()).max().unwrap_or(0);
        let width_of = |ls: &Vec<Line>| {
            ls.iter()
                .map(|l| {
                    l.frags
                        .iter()
                        .map(|f| f.x + frag_width(f))
                        .fold(0.0, f32::max)
                })
                .fold(0.0, f32::max)
        };
        let widths: Vec<f32> = (0..columns)
            .map(|c| {
                cells
                    .iter()
                    .filter_map(|r| r.get(c))
                    .map(width_of)
                    .fold(0.0, f32::max)
            })
            .collect();
        let gap = size * 1.5;
        let mut unit = Unit {
            space_before: self.leading(size) * 0.5,
            ..Default::default()
        };
        for row in cells {
            let mut line = Line {
                size,
                height: self.leading(size),
                block: block.to_string(),
                x: left,
                ..Default::default()
            };
            let mut x = 0.0;
            for (c, cell) in row.into_iter().enumerate() {
                if let Some(first) = cell.into_iter().next() {
                    for f in first.frags {
                        line.frags.push(Frag { x: x + f.x, ..f });
                    }
                    for (t, a, b) in first.words {
                        line.words.push((t, x + a, x + b));
                    }
                }
                x += widths.get(c).copied().unwrap_or(0.0) + gap;
            }
            let _ = measure;
            unit.lines.push(line);
        }
        unit
    }

    // ---- pages -----------------------------------------------------

    /// A chapter's units on pages, numbered, with each note's label in
    /// the chapter's notes linked back to the page of its marker.
    pub fn paginate(&self, units: &[Unit]) -> Vec<Page> {
        let p = self.profile;
        let area = Area {
            top: p.mm(p.margins.top),
            bottom: p.height as f32 - p.mm(p.margins.bottom) - self.body,
            left: p.mm(p.margins.left),
        };
        let mut pages = self.flow(units, area);
        // The page each note's marker is first on.
        let mut marker: std::collections::HashMap<String, usize> = Default::default();
        for (n, page) in pages.iter().enumerate() {
            for l in &page.links {
                if let Target::Note { id } = &l.to {
                    marker.entry(id.clone()).or_insert(n);
                }
            }
        }
        for page in &mut pages {
            page.links.retain_mut(|l| match &l.to {
                Target::Marker(label) => match marker.get(label) {
                    Some(&page) => {
                        l.to = Target::Page { page };
                        true
                    }
                    None => false,
                },
                _ => true,
            });
        }
        self.number(&mut pages);
        pages
    }

    fn flow(&self, units: &[Unit], area: Area) -> Vec<Page> {
        let Area { top, bottom, left } = area;
        let mut pages: Vec<Page> = vec![Page::default()];
        let mut y = top;
        let fits = |y: f32, h: f32| y + h <= bottom;
        for (k, u) in units.iter().enumerate() {
            let height: f32 = u.lines.iter().map(|l| l.height).sum();
            let at_top = y <= top + 0.5;
            let before = if at_top { 0.0 } else { u.space_before };
            // A heading wants the first lines of what follows with it.
            let follow: f32 = if u.keep_with_next {
                units.get(k + 1).map_or(0.0, |n| {
                    n.space_before + n.lines.iter().take(2).map(|l| l.height).sum::<f32>()
                })
            } else {
                0.0
            };
            let whole = before + height + follow;
            if !fits(y, whole)
                && !at_top
                && (!u.split
                    || !fits(
                        y,
                        before + u.lines.iter().take(2).map(|l| l.height).sum::<f32>(),
                    ))
                && (!u.split || height + follow <= bottom - top)
            {
                pages.push(Page::default());
                y = top;
            }
            y += if y <= top + 0.5 { 0.0 } else { u.space_before };
            for line in &u.lines {
                if !fits(y, line.height) && y > top + 0.5 {
                    pages.push(Page::default());
                    y = top;
                }
                let page = pages.last_mut().expect("one page at least");
                self.place(page, line, left, y);
                y += line.height;
            }
        }
        pages
    }

    fn number(&self, pages: &mut [Page]) {
        let p = self.profile;
        for (n, page) in pages.iter_mut().enumerate() {
            let style = Style {
                face: Face::Regular,
                size: self.body * 0.75,
                smcp: false,
            };
            let (frags, w) = self.frags(&(n + 1).to_string(), style, 0.0);
            let x = (p.width as f32 - w) / 2.0;
            let baseline = p.height as f32 - p.mm(p.margins.bottom) * 0.6;
            for f in frags {
                page.runs.push(run(&f, x, baseline));
            }
        }
    }

    fn place(&self, page: &mut Page, line: &Line, left: f32, top: f32) {
        let (ascent, descent) = self.fonts.ascent_descent(line.size);
        // Centre the line's ink in its slot.
        let baseline = top + (line.height - (ascent + descent)) / 2.0 + ascent;
        for f in &line.frags {
            page.runs.push(run(f, left + line.x, baseline));
        }
        for f in &line.marks {
            page.runs.push(run(f, left, baseline));
        }
        // A marker is a few pixels wide: the box to tap is wider.
        let reach = self.profile.mm(6.0);
        let notes = line
            .notes
            .iter()
            .map(|(id, x0, x1)| (id, line.x + x0, line.x + x1));
        let margin = line.margin_links.iter().map(|(id, x0, x1)| (id, *x0, *x1));
        for (id, x0, x1) in notes.chain(margin) {
            let middle = left + (x0 + x1) / 2.0;
            let half = ((x1 - x0) / 2.0).max(reach / 2.0);
            page.links.push(Link {
                to: Target::Note { id: id.clone() },
                x0: middle - half,
                y0: top,
                x1: middle + half,
                y1: top + line.height,
            });
        }
        if let Some((id, n)) = &line.back {
            if let (Some((_, x0, _)), Some((_, _, x1))) =
                (line.words.first(), line.words.get(n.saturating_sub(1)))
            {
                page.links.push(Link {
                    to: Target::Marker(id.clone()),
                    x0: left + line.x + x0,
                    y0: top,
                    x1: left + line.x + x1.max(x0 + reach),
                    y1: top + line.height,
                });
            }
        }
        for (text, x0, x1) in &line.words {
            page.words.push(Word {
                text: text.clone(),
                block: line.block.clone(),
                x0: left + line.x + x0,
                y0: baseline - ascent,
                x1: left + line.x + x1,
                y1: baseline + descent,
            });
        }
    }
}

/// The part of a page that text flows into.
#[derive(Clone, Copy)]
struct Area {
    top: f32,
    bottom: f32,
    left: f32,
}

fn run(f: &Frag, x: f32, baseline: f32) -> Run {
    Run {
        font: f.font,
        size: f.size,
        x: x + f.x,
        y: baseline + f.dy,
        glyphs: f.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
    }
}

fn frag_width(f: &Frag) -> f32 {
    // The last glyph's position is close enough for centring.
    f.glyphs.last().map_or(0.0, |g| g.x + f.size * 0.5)
}

/// Greedy breaks: fill each line, and break at the last place that fits
/// when the next word doesn't.
fn greedy(items: &[Item], measure: f32) -> (Vec<usize>, Vec<f32>) {
    let mut breaks = Vec::new();
    // Width so far, where the current line starts, and the last place it
    // could break (with where the next line would then start).
    let mut total = 0.0f32;
    let mut start = 0.0f32;
    let mut last: Option<(usize, f32)> = None;
    for (k, item) in items.iter().enumerate() {
        match *item {
            Item::Box { width } => {
                total += width;
                if total - start > measure {
                    if let Some((b, next)) = last.take() {
                        breaks.push(b);
                        start = next;
                    }
                }
            }
            Item::Glue { width, .. } => {
                last = Some((k, total + width));
                total += width;
            }
            Item::Penalty { cost, .. } if cost <= -INFINITY => {
                breaks.push(k);
                start = total;
                last = None;
            }
            Item::Penalty { cost, .. } if cost < INFINITY => last = Some((k, total)),
            Item::Penalty { .. } => {}
        }
    }
    let ratios = vec![0.0; breaks.len()];
    (breaks, ratios)
}
