//! An edition's XHTML, walked in reading order and classified by its
//! recipe. What the recipe doesn't cover is reported, not guessed.

use std::collections::BTreeMap;

use regex::Regex;
use roxmltree::{Node, NodeId};
use wordhord_book::model::Inline;

use crate::recipe::{Kind, Recipe, Role, Selector};

/// A block as imported, before it's written as Djot.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Heading {
        level: u16,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    /// A stanza, or a group of lines without a number.
    Stanza {
        number: Option<u32>,
        lines: Vec<Line>,
    },
    /// A note on a passage: paragraphs, and any verse it quotes.
    Commentary {
        on: String,
        blocks: Vec<Out>,
    },
    /// A thematic break: a rule or row of dots between sections.
    Break,
    /// A poem or part of one (a canto, a book): groups of lines, counted
    /// from `start` under `reference` when the work numbers its lines.
    Verse {
        reference: Option<String>,
        start: Option<u32>,
        groups: Vec<Vec<Line>>,
    },
    /// A block quotation.
    Quote(Vec<Out>),
    /// A table: rows of cells.
    Table(Vec<Vec<Vec<Inline>>>),
    /// A speech: who speaks, and what they say (a row of a drama table).
    Speech {
        speaker: Option<String>,
        blocks: Vec<Out>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub indent: u8,
    pub inlines: Vec<Inline>,
}

/// One work: a chapter of the output.
#[derive(Debug, Clone)]
pub struct Chapter {
    pub prefix: String,
    pub blocks: Vec<Out>,
    /// Footnotes, by label, in order: each a list of blocks.
    pub notes: Vec<(String, Vec<Out>)>,
}

/// What the import found that the recipe doesn't explain.
#[derive(Debug, Default)]
pub struct Report {
    /// `tag.class` → how often, and a sample of its text.
    pub unmapped: BTreeMap<String, (usize, String)>,
    /// Elements the converter doesn't handle yet, and how often.
    pub unhandled: BTreeMap<String, (usize, String)>,
    pub problems: Vec<String>,
    pub pages: usize,
    /// Stanzas that continue the one before, after a change of speaker.
    pub continued: usize,
    /// Verse the edition didn't number: kept as verse, with no reference.
    pub unnumbered: usize,
}

struct CompiledRole<'r> {
    selector: Selector,
    role: &'r Role,
    pattern: Option<Regex>,
    number: Option<Selector>,
    target: Option<Selector>,
}

pub struct Converter<'r, 'd> {
    recipe: &'r Recipe,
    /// Every document, parsed up front, so a note reference can reach a
    /// note in another file.
    docs: &'d [roxmltree::Document<'d>],
    /// Element IDs, to their document and node.
    ids: std::collections::HashMap<&'d str, (usize, NodeId)>,
    /// Inside a footnote or quotation: blocks go to a capture, and source
    /// IDs aren't recorded as targets.
    capturing: usize,
    roles: Vec<CompiledRole<'r>>,
    pub chapters: Vec<Chapter>,
    pub report: Report,
    active: bool,
    done: bool,
    /// A source page that started between blocks: it goes on the next one.
    pending_page: Vec<Inline>,
    /// Lines outside any stanza element, gathered into the open group.
    loose_open: bool,
    /// Source element IDs, to the output block they became: `#ch04-0007`.
    pub targets: std::collections::HashMap<String, String>,
    /// The next work to look for: works start in the recipe's order.
    next_work: usize,
    /// Which headings can start or end a work.
    starts: Option<Selector>,
}

const CONTAINERS: &[&str] = &[
    "body", "div", "section", "article", "header", "hgroup", "main", "aside", "footer", "td", "th",
];

impl<'r, 'd> Converter<'r, 'd> {
    pub fn new(recipe: &'r Recipe, docs: &'d [roxmltree::Document<'d>]) -> Result<Self, String> {
        if let Some(p) = &recipe.caesura_pattern {
            Regex::new(p).map_err(|e| format!("caesura_pattern `{p}`: {e}"))?;
        }
        let mut roles = Vec::new();
        for role in &recipe.roles {
            let pattern = match &role.pattern {
                Some(p) => Some(Regex::new(p).map_err(|e| format!("pattern `{p}`: {e}"))?),
                None => None,
            };
            let number = match &role.number {
                Some(s) => Some(Selector::parse(s)?),
                None => None,
            };
            let target = match &role.target {
                Some(s) => Some(Selector::parse(s)?),
                None => None,
            };
            roles.push(CompiledRole {
                selector: Selector::parse(&role.select)?,
                role,
                pattern,
                number,
                target,
            });
        }
        let mut ids = std::collections::HashMap::new();
        for (k, doc) in docs.iter().enumerate() {
            for n in doc.descendants() {
                if let Some(id) = n.attribute("id") {
                    ids.entry(id).or_insert((k, n.id()));
                }
            }
        }
        Ok(Converter {
            recipe,
            docs,
            ids,
            capturing: 0,
            roles,
            chapters: Vec::new(),
            report: Report::default(),
            active: false,
            done: false,
            pending_page: Vec::new(),
            loose_open: false,
            targets: std::collections::HashMap::new(),
            next_work: 0,
            starts: match &recipe.starts {
                Some(s) => Some(Selector::parse(s)?),
                None => None,
            },
        })
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    fn role(&self, node: Node<'d, 'd>) -> Option<&CompiledRole<'r>> {
        self.roles.iter().find(|r| r.selector.matches(node))
    }

    fn note(map: &mut BTreeMap<String, (usize, String)>, key: String, node: Node) {
        let e = map.entry(key).or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() {
            e.1 = collapse(
                &node
                    .descendants()
                    .filter(|n| n.is_text())
                    .filter_map(|n| n.text())
                    .collect::<String>(),
            )
            .chars()
            .take(60)
            .collect();
        }
    }

    /// Records classes the recipe neither maps nor calls styling.
    fn unmapped(&mut self, node: Node<'d, 'd>) {
        if !self.active {
            return;
        }
        for c in node
            .attribute("class")
            .unwrap_or_default()
            .split_whitespace()
        {
            if !self.recipe.styling.iter().any(|s| s == c) && !self.recipe.indent.contains_key(c) {
                Self::note(
                    &mut self.report.unmapped,
                    format!("{}.{c}", node.tag_name().name()),
                    node,
                );
            }
        }
    }

    fn push(&mut self, block: Out) {
        if let Some(c) = self.chapters.last_mut() {
            c.blocks.push(block);
        }
        self.loose_open = false;
    }

    /// Pushes a block made from `node`, remembering where its source ID went.
    fn push_from(&mut self, node: Node<'d, 'd>, block: Out) {
        self.push(block);
        if self.capturing > 0 {
            return;
        }
        if let (Some(id), Some(c)) = (node.attribute("id"), self.chapters.last()) {
            let target = format!("#ch{:02}-{:04}", self.chapters.len(), c.blocks.len());
            self.targets.insert(id.to_string(), target);
        }
    }

    /// Walks the `k`th document's body.
    pub fn document(&mut self, k: usize) {
        let doc: &'d roxmltree::Document<'d> = &self.docs[k];
        if let Some(body) = doc.descendants().find(|n| n.has_tag_name("body")) {
            self.blocks(body);
        }
    }

    /// The blocks `f` makes, taken aside instead of into the chapter: a
    /// footnote's body, a quotation's contents.
    fn capture(&mut self, f: impl FnOnce(&mut Self)) -> Vec<Out> {
        let Some(chapter) = self.chapters.last_mut() else {
            return Vec::new();
        };
        let saved = std::mem::take(&mut chapter.blocks);
        let loose = self.loose_open;
        let page = std::mem::take(&mut self.pending_page);
        let active = self.active;
        self.active = true;
        self.capturing += 1;
        f(self);
        self.capturing -= 1;
        self.active = active;
        self.loose_open = loose;
        let leftover = std::mem::replace(&mut self.pending_page, page);
        let chapter = self.chapters.last_mut().expect("still there");
        let mut got = std::mem::replace(&mut chapter.blocks, saved);
        // A page marker at the very end of a capture stays in it.
        if !leftover.is_empty() {
            got.push(Out::Paragraph(leftover));
        }
        got
    }

    fn blocks(&mut self, node: Node<'d, 'd>) {
        for child in node.children() {
            if self.done {
                return;
            }
            if child.is_text() {
                if self.active && !collapse(child.text().unwrap_or_default()).is_empty() {
                    Self::note(
                        &mut self.report.unhandled,
                        "text between blocks".into(),
                        node,
                    );
                }
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let tag = child.tag_name().name();
            let kind = self.role(child).map(|r| r.role.kind).or_else(|| match tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(Kind::Heading),
                "p" => Some(Kind::Paragraph),
                "blockquote" => Some(Kind::Quote),
                // An attribution under a quotation.
                "cite" => Some(Kind::Paragraph),
                "table" => Some(Kind::Table),
                t if CONTAINERS.contains(&t) => Some(Kind::Container),
                _ => None,
            });
            if self.role(child).is_none() && kind.is_some() {
                self.unmapped(child);
            }
            match kind {
                Some(Kind::Heading) => self.heading(child),
                Some(Kind::Container | Kind::Unwrap) => self.blocks(child),
                Some(Kind::Drop) => {}
                // A poem's heading may be what starts a work: walk it
                // even before one has started.
                Some(Kind::Poem) => self.poem(child),
                _ if !self.active => {}
                Some(Kind::Paragraph) => self.paragraph(child),
                Some(Kind::Stanza) => self.stanza(child),
                Some(Kind::Verse) => self.quoted_verse(child),
                Some(Kind::Gap) => {
                    let mut inlines = std::mem::take(&mut self.pending_page);
                    inlines.extend(self.gap(child));
                    self.push(Out::Paragraph(inlines));
                }
                Some(Kind::Break) => self.push(Out::Break),
                Some(Kind::Line) => self.loose_line(child),
                Some(Kind::Commentary) => self.commentary(child),
                Some(Kind::Page) => {
                    if let Some(p) = self.page(child) {
                        self.pending_page.push(p);
                    }
                }
                Some(Kind::Quote) => {
                    let inner = self.capture(|c| c.blocks(child));
                    if !inner.is_empty() {
                        let mut page = std::mem::take(&mut self.pending_page);
                        if !page.is_empty() {
                            page.extend(Vec::<Inline>::new());
                            self.push(Out::Paragraph(page));
                        }
                        self.push(Out::Quote(inner));
                    }
                }
                Some(Kind::Table) => {
                    let rows = self.table(child);
                    self.push(Out::Table(rows));
                }
                Some(Kind::Drama) => self.drama(child),
                // A speaker is taken by its drama row.
                Some(Kind::Speaker) => {}
                Some(Kind::Emphasis | Kind::Strong | Kind::Corr | Kind::Note | Kind::NoteRef) => {
                    Self::note(
                        &mut self.report.unhandled,
                        format!("an inline role between blocks: <{tag}>"),
                        child,
                    )
                }
                None => Self::note(&mut self.report.unhandled, format!("<{tag}>"), child),
            }
        }
    }

    /// A footnote in the text: its body goes to the chapter's notes, and a
    /// reference to it is returned for the text.
    fn footnote(&mut self, node: Node<'d, 'd>) -> Option<Inline> {
        let blockish = node.children().any(|n| {
            n.is_element()
                && matches!(
                    n.tag_name().name(),
                    "p" | "blockquote" | "table" | "div" | "ol" | "ul"
                )
        });
        let body = if blockish {
            self.capture(|c| c.blocks(node))
        } else {
            vec![Out::Paragraph(tidy(self.inlines(node, None)))]
        };
        let wanted = node.attribute("n").map(String::from);
        self.add_note(wanted, body)
    }

    /// A reference to a note elsewhere (an endnote): the note it links to
    /// becomes a footnote here.
    fn endnote(&mut self, node: Node<'d, 'd>) -> Option<Inline> {
        let href = node.attribute("href").unwrap_or_default();
        let fragment = href.split_once('#').map_or(href, |(_, f)| f);
        let Some(&(k, id)) = self.ids.get(fragment) else {
            self.report.problems.push(format!(
                "a note reference to #{fragment}, which isn't in the book"
            ));
            return None;
        };
        let doc: &'d roxmltree::Document<'d> = &self.docs[k];
        let target = doc.get_node(id)?;
        let body = self.capture(|c| c.blocks(target));
        // Text nodes only: an element's text() is its first child's again.
        let label = collapse(
            &node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>(),
        );
        let wanted = (!label.is_empty()).then_some(label);
        self.add_note(wanted, body)
    }

    fn add_note(&mut self, wanted: Option<String>, body: Vec<Out>) -> Option<Inline> {
        let chapter = self.chapters.last_mut()?;
        let wanted = wanted.unwrap_or_else(|| (chapter.notes.len() + 1).to_string());
        // Labels are unique within a chapter.
        let mut label = wanted.clone();
        let mut k = 1;
        while chapter.notes.iter().any(|(l, _)| *l == label) {
            k += 1;
            label = format!("{wanted}-{k}");
        }
        chapter.notes.push((label.clone(), body));
        Some(Inline::NoteRef(label))
    }

    /// A poem, or a canto or book of one: its heading, then groups of
    /// lines. Numbered if the recipe gives it a `number` (the canto's).
    fn poem(&mut self, node: Node<'d, 'd>) {
        let selector = self.role(node).and_then(|r| r.number.clone());
        let (text, _) = self.number(node, selector.as_ref());
        let number = text.as_deref().and_then(first_number);
        if selector.is_some() && number.is_none() {
            self.report
                .problems
                .push(format!("a poem without a number: {}", sample(node)));
        }
        let prefix = self
            .chapters
            .last()
            .map(|c| c.prefix.clone())
            .unwrap_or_default();
        let reference = number.map(|n| format!("{prefix}.{n}"));
        let mut groups: Vec<Vec<Line>> = Vec::new();
        // Lines gathered so far become a verse block. Only the poem's first
        // block carries its reference; a heading in between splits it.
        let mut numbered = false;
        let mut flush = |c: &mut Self, groups: &mut Vec<Vec<Line>>| {
            if groups.is_empty() {
                return;
            }
            let (r, s) = match &reference {
                Some(r) if !numbered => (Some(r.clone()), Some(1)),
                _ => (None, None),
            };
            numbered = true;
            c.push_from(
                node,
                Out::Verse {
                    reference: r,
                    start: s,
                    groups: std::mem::take(groups),
                },
            );
        };
        for child in node.children() {
            if !child.is_element() {
                if child.text().is_some_and(|t| !t.trim().is_empty()) {
                    Self::note(
                        &mut self.report.unhandled,
                        "text in a poem, outside its lines".into(),
                        node,
                    );
                }
                continue;
            }
            let tag = child.tag_name().name();
            match self.role(child).map(|r| r.role.kind) {
                Some(Kind::Drop) => {}
                Some(Kind::Page) => {
                    if let Some(p) = self.page(child) {
                        self.pending_page.push(p);
                    }
                }
                Some(Kind::Heading) => {
                    flush(self, &mut groups);
                    self.heading(child);
                }
                _ if matches!(
                    tag,
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hgroup"
                ) =>
                {
                    flush(self, &mut groups);
                    self.unmapped(child);
                    self.blocks(child);
                }
                // Lines count only once a work has started.
                Some(Kind::Paragraph) | None if tag == "p" && !self.active => {}
                Some(Kind::Paragraph) | None if tag == "p" => {
                    self.unmapped(child);
                    let lines = self.lines_of(child, None);
                    if lines.is_empty() {
                        Self::note(
                            &mut self.report.unhandled,
                            "a group of lines with no lines".into(),
                            child,
                        );
                    } else {
                        groups.push(lines);
                    }
                }
                other => {
                    flush(self, &mut groups);
                    Self::note(
                        &mut self.report.unhandled,
                        format!("{other:?} in a poem: <{tag}>"),
                        child,
                    );
                }
            }
        }
        flush(self, &mut groups);
    }

    /// A drama table: each row a speech, its speaker from the cell with
    /// the `speaker` role, its words from the rest (verse cells are poems).
    fn drama(&mut self, node: Node<'d, 'd>) {
        let rows: Vec<Node<'d, 'd>> = node
            .descendants()
            .filter(|n| {
                n.has_tag_name("tr")
                    && n.ancestors().find(|a| a.has_tag_name("table")) == Some(node)
            })
            .collect();
        for row in rows {
            let speaker = row
                .descendants()
                .find(|n| self.role(*n).is_some_and(|r| r.role.kind == Kind::Speaker))
                .map(|n| {
                    collapse(
                        &n.descendants()
                            .filter(|t| t.is_text())
                            .filter_map(|t| t.text())
                            .collect::<String>(),
                    )
                })
                .filter(|s| !s.is_empty());
            // A cell of plain prose has no paragraph of its own.
            let blocks = self.capture(|c| {
                for cell in row.children().filter(|n| n.is_element()) {
                    if c.role(cell).is_some_and(|r| r.role.kind == Kind::Speaker) {
                        continue;
                    }
                    let blockish = cell.children().any(|n| {
                        n.is_element()
                            && matches!(n.tag_name().name(), "p" | "blockquote" | "div" | "table")
                    });
                    if c.role(cell).is_some_and(|r| r.role.kind == Kind::Poem) {
                        c.poem(cell);
                    } else if blockish {
                        c.blocks(cell);
                    } else {
                        let inlines = tidy(c.inlines(cell, None));
                        if !inlines.is_empty() {
                            c.push(Out::Paragraph(inlines));
                        }
                    }
                }
            });
            self.push(Out::Speech { speaker, blocks });
        }
    }

    /// A table's rows and cells.
    fn table(&mut self, node: Node<'d, 'd>) -> Vec<Vec<Vec<Inline>>> {
        let rows: Vec<Node> = node
            .descendants()
            .filter(|n| {
                n.has_tag_name("tr")
                    && n.ancestors().find(|a| a.has_tag_name("table")) == Some(node)
            })
            .collect();
        rows.into_iter()
            .map(|r| {
                r.children()
                    .filter(|c| c.has_tag_name("td") || c.has_tag_name("th"))
                    .map(|cell| tidy(self.inlines(cell, None)))
                    .collect()
            })
            .collect()
    }

    /// A heading's text for matching, leaving out what the recipe drops
    /// and any footnote in it.
    fn heading_text(&self, node: Node<'d, 'd>) -> String {
        let dropped = |n: &Node| {
            n.ancestors().any(|a| {
                self.role(a)
                    .is_some_and(|r| matches!(r.role.kind, Kind::Drop | Kind::Note | Kind::NoteRef))
            })
        };
        collapse(
            &node
                .descendants()
                .filter(|n| n.is_text() && !dropped(n))
                .filter_map(|n| n.text())
                .collect::<String>(),
        )
    }

    fn heading(&mut self, node: Node<'d, 'd>) {
        // Only the edition's section headings start or end works, not a
        // chapter title or a synopsis subheading that shares the name.
        let text = if self.starts.as_ref().is_none_or(|s| s.matches(node)) {
            self.heading_text(node)
        } else {
            String::new()
        };
        if self.recipe.end.as_deref() == Some(text.as_str()) && self.active {
            self.done = true;
            return;
        }
        // The current work's own end: stop taking text until the next work.
        let current = self
            .next_work
            .checked_sub(1)
            .and_then(|k| self.recipe.works.get(k));
        if self.active && current.and_then(|w| w.end.as_deref()) == Some(text.as_str()) {
            self.active = false;
            return;
        }
        if let Some(w) = self
            .recipe
            .works
            .get(self.next_work)
            .filter(|w| w.start == text)
        {
            self.chapters.push(Chapter {
                prefix: w.prefix.clone(),
                blocks: Vec::new(),
                notes: Vec::new(),
            });
            self.next_work += 1;
            self.active = true;
            self.pending_page.clear();
        }
        if !self.active {
            return;
        }
        let level = self
            .role(node)
            .and_then(|r| r.role.level)
            .unwrap_or_else(|| {
                node.tag_name()
                    .name()
                    .strip_prefix('h')
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(2)
            });
        let mut inlines = std::mem::take(&mut self.pending_page);
        inlines.extend(self.inlines(node, None));
        self.push(Out::Heading {
            level,
            inlines: tidy(inlines),
        });
    }

    fn paragraph(&mut self, node: Node<'d, 'd>) {
        let inlines = tidy(self.inlines(node, None));
        if inlines
            .iter()
            .all(|i| matches!(i, Inline::PageBreak { .. }))
        {
            self.pending_page.extend(inlines);
            return;
        }
        let mut all = std::mem::take(&mut self.pending_page);
        all.extend(inlines);
        self.push_from(node, Out::Paragraph(all));
    }

    /// The number an element carries, its node (to leave out of the text),
    /// and the target of any link in it: Bellows' note labels link to what
    /// they're about.
    fn number(
        &self,
        node: Node<'d, 'd>,
        selector: Option<&Selector>,
    ) -> (Option<String>, Option<NodeId>) {
        let Some(sel) = selector else {
            return (None, None);
        };
        match node.descendants().find(|n| sel.matches(*n)) {
            Some(n) => {
                let text = collapse(
                    &n.descendants()
                        .filter(|n| n.is_text())
                        .filter_map(|n| n.text())
                        .collect::<String>(),
                );
                (Some(text), Some(n.id()))
            }
            None => (None, None),
        }
    }

    fn stanza(&mut self, node: Node<'d, 'd>) {
        let role = self.role(node).expect("a stanza has a role");
        let selector = role.number.clone();
        let (text, skip) = self.number(node, selector.as_ref());
        let mut number = text.as_deref().and_then(first_number);
        if selector.is_some() && number.is_none() {
            // Bellows splits a stanza at a change of speaker: the second
            // part has no number of its own.
            match self.last_number() {
                Some(n) => {
                    number = Some(n);
                    self.report.continued += 1;
                }
                // Verse quoted in prose, before the poem's first stanza:
                // the editor didn't number it, so it gets no reference.
                None => self.report.unnumbered += 1,
            }
        }
        let lines = self.lines_of(node, skip);
        if lines.is_empty() {
            self.report.problems.push(format!(
                "a stanza with no lines the recipe recognises: {}",
                sample(node)
            ));
            return;
        }
        self.push_from(node, Out::Stanza { number, lines });
    }

    /// The number of the last numbered stanza in this work.
    fn last_number(&self) -> Option<u32> {
        self.chapters
            .last()?
            .blocks
            .iter()
            .rev()
            .find_map(|b| match b {
                Out::Stanza { number, .. } => *number,
                _ => None,
            })
    }

    /// Lines with no stanza number: verse quoted in a note goes into the
    /// note; elsewhere it's a verse block of its own.
    fn quoted_verse(&mut self, node: Node<'d, 'd>) {
        let lines = self.lines_of(node, None);
        if lines.is_empty() {
            self.report.problems.push(format!(
                "verse with no lines the recipe recognises: {}",
                sample(node)
            ));
            return;
        }
        let verse = Out::Stanza {
            number: None,
            lines,
        };
        match self.chapters.last_mut().and_then(|c| c.blocks.last_mut()) {
            Some(Out::Commentary { blocks, .. }) => blocks.push(verse),
            _ => self.push(verse),
        }
    }

    /// The lines inside an element, not counting lines inside lines.
    fn lines_of(&mut self, node: Node<'d, 'd>, skip: Option<NodeId>) -> Vec<Line> {
        let mut lines = Vec::new();
        self.walk_lines(node, skip, &mut lines);
        lines
    }

    /// A stanza's contents in order. Lines become lines; a source page
    /// goes on the next line; a gap is a line of its own; anything else
    /// is reported, so nothing in a stanza is dropped unseen.
    fn walk_lines(&mut self, node: Node<'d, 'd>, skip: Option<NodeId>, lines: &mut Vec<Line>) {
        for child in node.children() {
            if Some(child.id()) == skip {
                continue;
            }
            if child.is_text() {
                if !collapse(child.text().unwrap_or_default()).is_empty() {
                    Self::note(
                        &mut self.report.unhandled,
                        "text in a stanza, outside its lines".into(),
                        node,
                    );
                }
                continue;
            }
            if !child.is_element() {
                continue;
            }
            match self.role(child).map(|r| r.role.kind) {
                Some(Kind::Line) => {
                    let mut inlines = std::mem::take(&mut self.pending_page);
                    inlines.extend(self.inlines(child, skip));
                    lines.push(Line {
                        indent: self.indent(child),
                        inlines: self.verse_inlines(inlines),
                    });
                }
                Some(Kind::Page) => {
                    if let Some(p) = self.page(child) {
                        self.pending_page.push(p);
                    }
                }
                Some(Kind::Gap) => {
                    let mut inlines = std::mem::take(&mut self.pending_page);
                    inlines.extend(self.gap(child));
                    lines.push(Line { indent: 0, inlines });
                }
                Some(Kind::Drop) => {}
                Some(Kind::Container | Kind::Unwrap) | None
                    if !matches!(child.tag_name().name(), "p") =>
                {
                    self.unmapped(child);
                    self.walk_lines(child, skip, lines);
                }
                other => Self::note(
                    &mut self.report.unhandled,
                    format!("{other:?} in a stanza: {}", child.tag_name().name()),
                    child,
                ),
            }
        }
    }

    /// Text lost from the source, as the edition marks it: the mark in a
    /// gap span, with any source page inside it moved out in front.
    fn gap(&mut self, node: Node<'d, 'd>) -> Vec<Inline> {
        let inlines = tidy(self.inlines(node, None));
        let (pages, mark): (Vec<Inline>, Vec<Inline>) = inlines
            .into_iter()
            .partition(|i| matches!(i, Inline::PageBreak { .. }));
        let mut out = pages;
        out.push(Inline::Span {
            kind: wordhord_book::model::SpanKind::Gap,
            lang: None,
            inlines: tidy(mark),
        });
        out
    }

    /// A line outside any stanza element. If it carries a number (the
    /// recipe's `number` on its line role), it opens a new stanza;
    /// otherwise it joins the open one.
    fn loose_line(&mut self, node: Node<'d, 'd>) {
        let selector = self.role(node).and_then(|r| r.number.clone());
        let (text, skip) = self.number(node, selector.as_ref());
        let number = text.as_deref().and_then(first_number);
        let mut inlines = std::mem::take(&mut self.pending_page);
        let mut own = self.inlines(node, skip);
        if skip.is_some() {
            // The number's period, when it sits outside the number: `1</ref>.`
            let first_words =
                |i: &&mut Inline| matches!(i, Inline::Text(t) if !t.trim().is_empty());
            if let Some(Inline::Text(t)) = own.iter_mut().find(first_words) {
                let trimmed = t.trim_start();
                if let Some(rest) = trimmed.strip_prefix('.') {
                    *t = rest.to_string();
                }
            }
        }
        inlines.append(&mut own);
        let line = Line {
            indent: self.indent(node),
            inlines: self.verse_inlines(inlines),
        };
        let open = self.loose_open;
        match self.chapters.last_mut().and_then(|c| c.blocks.last_mut()) {
            Some(Out::Stanza { lines, .. }) if open && number.is_none() => lines.push(line),
            _ => {
                self.push(Out::Stanza {
                    number,
                    lines: vec![line],
                });
                self.loose_open = true;
            }
        }
    }

    fn commentary(&mut self, node: Node<'d, 'd>) {
        let role = self.role(node).expect("a commentary has a role");
        let selector = role.number.clone();
        let role_target = role.target.clone();
        let (text, mut skip) = self.number(node, selector.as_ref());
        // No number: a label that links to the note's subject instead.
        if text.is_none() {
            if let Some((label, _)) = target_link(node, role_target.as_ref()) {
                skip = Some(label);
            }
        }
        let body = tidy(self.inlines(node, skip));
        // A page number alone between notes: it goes on the next block.
        if body.iter().all(|i| matches!(i, Inline::PageBreak { .. })) {
            self.pending_page.extend(body);
            return;
        }
        let mut paragraph = std::mem::take(&mut self.pending_page);
        paragraph.extend(body);
        let paragraph = tidy(paragraph);
        let prefix = self
            .chapters
            .last()
            .map(|c| c.prefix.clone())
            .unwrap_or_default();
        let on = match text.as_deref().and_then(number_range) {
            Some(r) => Some(format!("{prefix}.{r}")),
            // A note on something without a number (Bellows' "Prose."):
            // its label links to it.
            None if target_link(node, role_target.as_ref()).is_some() => {
                let link = target_link(node, role_target.as_ref());
                let link = link.map(|(_, f)| f);
                match link.as_ref().and_then(|f| self.targets.get(f)) {
                    Some(t) => Some(t.clone()),
                    None => {
                        self.report.problems.push(format!(
                            "a note on {:?}, which isn't a block the import made: {}",
                            link.unwrap_or_default(),
                            sample(node)
                        ));
                        return;
                    }
                }
            }
            None => None,
        };
        match on {
            Some(on) => self.push(Out::Commentary {
                on,
                blocks: vec![Out::Paragraph(paragraph)],
            }),
            None => match self.chapters.last_mut().and_then(|c| c.blocks.last_mut()) {
                // A note that runs on for another paragraph.
                Some(Out::Commentary { blocks, .. }) => blocks.push(Out::Paragraph(paragraph)),
                _ => self
                    .report
                    .problems
                    .push(format!("a note with no stanza number: {}", sample(node))),
            },
        }
    }

    fn page(&mut self, node: Node<'d, 'd>) -> Option<Inline> {
        let role = self.role(node)?;
        let source = match role.role.from.as_deref() {
            None | Some("text") => collapse(
                &node
                    .descendants()
                    .filter(|n| n.is_text())
                    .filter_map(|n| n.text())
                    .collect::<String>(),
            ),
            Some(attr) => crate::recipe::attribute(node, attr)
                .unwrap_or_default()
                .to_string(),
        };
        let n = match &role.pattern {
            Some(p) => p
                .captures(&source)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string()),
            None => Some(source),
        }?;
        self.report.pages += 1;
        Some(Inline::PageBreak { n, edition: None })
    }

    fn indent(&self, node: Node<'d, 'd>) -> u8 {
        node.attribute("class")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|c| self.recipe.indent.get(c))
            .copied()
            .max()
            .unwrap_or(0)
    }

    fn verse_inlines(&self, inlines: Vec<Inline>) -> Vec<Inline> {
        let inlines = tidy(inlines);
        // Editions pad the bar with ordinary or non-breaking spaces; some
        // have no bar, only a run of spaces.
        let bar = match (&self.recipe.caesura_pattern, self.recipe.caesura) {
            (Some(p), _) => Regex::new(p).expect("checked when the recipe was read"),
            (None, true) => Regex::new(r"[\s\u{a0}]+\|[\s\u{a0}]+").expect("valid"),
            (None, false) => return inlines,
        };
        let mut out = Vec::new();
        for i in inlines {
            match i {
                Inline::Text(t) if bar.is_match(&t) => {
                    let mut last = 0;
                    for m in bar.find_iter(&t) {
                        if m.start() > last {
                            out.push(Inline::Text(t[last..m.start()].to_string()));
                        }
                        out.push(Inline::Caesura);
                        last = m.end();
                    }
                    if last < t.len() {
                        out.push(Inline::Text(t[last..].to_string()));
                    }
                }
                other => out.push(other),
            }
        }
        out
    }

    /// An element's inline content. `skip` leaves out a number already
    /// taken from it.
    fn inlines(&mut self, node: Node<'d, 'd>, skip: Option<NodeId>) -> Vec<Inline> {
        let mut out = Vec::new();
        for child in node.children() {
            if Some(child.id()) == skip {
                continue;
            }
            if child.is_text() {
                out.push(Inline::Text(child.text().unwrap_or_default().to_string()));
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let kind = self.role(child).map(|r| r.role.kind);
            match kind {
                Some(Kind::Page) => {
                    if let Some(p) = self.page(child) {
                        out.push(p);
                    }
                }
                Some(Kind::Drop) => {}
                Some(Kind::Unwrap) => out.extend(self.inlines(child, skip)),
                Some(Kind::Emphasis) => out.push(Inline::Emphasis(self.inlines(child, skip))),
                Some(Kind::Strong) => out.push(Inline::Strong(self.inlines(child, skip))),
                Some(Kind::Note) => {
                    if let Some(r) = self.footnote(child) {
                        out.push(r);
                    }
                }
                Some(Kind::NoteRef) => {
                    if let Some(r) = self.endnote(child) {
                        out.push(r);
                    }
                }
                Some(Kind::Corr) => out.push(Inline::Span {
                    kind: wordhord_book::model::SpanKind::Corr,
                    lang: None,
                    inlines: self.inlines(child, skip),
                }),
                Some(_) => {
                    Self::note(
                        &mut self.report.unhandled,
                        format!("a block role inside text: {}", child.tag_name().name()),
                        child,
                    );
                    out.extend(self.inlines(child, skip));
                }
                None => match child.tag_name().name() {
                    "i" | "em" | "cite" => {
                        self.unmapped(child);
                        let i = Inline::Emphasis(self.inlines(child, skip));
                        out.push(self.in_language(child, i));
                    }
                    "b" | "strong" => {
                        self.unmapped(child);
                        out.push(Inline::Strong(self.inlines(child, skip)));
                    }
                    "br" => out.push(Inline::Text(" ".into())),
                    "a" | "span" | "small" | "abbr" | "q" => {
                        self.unmapped(child);
                        let inner = self.inlines(child, skip);
                        match self.language(child) {
                            Some(lang) => out.push(Inline::Span {
                                kind: wordhord_book::model::SpanKind::Foreign,
                                lang: Some(lang),
                                inlines: inner,
                            }),
                            None => out.extend(inner),
                        }
                    }
                    other => {
                        Self::note(
                            &mut self.report.unhandled,
                            format!("<{other}> in text"),
                            child,
                        );
                        out.extend(self.inlines(child, skip));
                    }
                },
            }
        }
        out
    }
}

impl<'r, 'd> Converter<'r, 'd> {
    /// An element's language, as ISO 639-3, when the recipe asks for
    /// languages and the element names one.
    fn language(&mut self, node: Node<'d, 'd>) -> Option<String> {
        if !self.recipe.languages {
            return None;
        }
        let code = node
            .attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
            .or_else(|| node.attribute("lang"))?;
        match iso639_3(code) {
            Some(c) => Some(c),
            None => {
                Self::note(
                    &mut self.report.unhandled,
                    format!("language {code:?}"),
                    node,
                );
                None
            }
        }
    }

    fn in_language(&mut self, node: Node<'d, 'd>, inline: Inline) -> Inline {
        match self.language(node) {
            Some(lang) => Inline::Span {
                kind: wordhord_book::model::SpanKind::Foreign,
                lang: Some(lang),
                inlines: vec![inline],
            },
            None => inline,
        }
    }
}

/// Language tags as EPUBs write them (BCP 47, usually two letters), as the
/// vocabulary's ISO 639-3. Unknown ones are reported, not guessed.
fn iso639_3(tag: &str) -> Option<String> {
    let primary = tag
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    // Three letters are already ISO 639 (Lombard `lmo`, Emilian `egl`).
    if primary.len() == 3 && primary.bytes().all(|b| b.is_ascii_lowercase()) {
        return Some(primary);
    }
    let code = match primary.as_str() {
        "en" | "eng" => "eng",
        "it" | "ita" => "ita",
        "la" | "lat" => "lat",
        "fr" | "fra" | "fre" => "fra",
        "de" | "deu" | "ger" => "deu",
        "es" | "spa" => "spa",
        "pt" | "por" => "por",
        "el" | "ell" => "ell",
        "grc" => "grc",
        "he" | "heb" => "heb",
        "ar" | "ara" => "ara",
        "enm" => "enm",
        "ang" => "ang",
        "frm" => "frm",
        "fro" => "fro",
        "pro" => "pro",
        "oc" | "oci" => "oci",
        "non" => "non",
        "is" | "isl" => "isl",
        "nl" | "nld" => "nld",
        "co" => "cos",
        "ro" => "ron",
        "ca" => "cat",
        "ru" => "rus",
        "sv" => "swe",
        "da" => "dan",
        "no" => "nor",
        _ => return None,
    };
    Some(code.to_string())
}

/// Whitespace runs as one space, and trimmed.
pub fn collapse(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() && c != '\u{a0}')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

enum Wrap {
    Emphasis,
    Strong,
    Span(wordhord_book::model::SpanKind, Option<String>),
}

/// Joins text, collapses whitespace inside it, trims the ends, and drops
/// what's left empty.
pub fn tidy(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut flat: Vec<Inline> = Vec::new();
    for i in inlines {
        // What wraps the words: italics, bold, or a span.
        let (inner, wrap) = match i {
            Inline::Emphasis(c) => (c, Wrap::Emphasis),
            Inline::Strong(c) => (c, Wrap::Strong),
            Inline::Span {
                kind,
                lang,
                inlines,
            } => (inlines, Wrap::Span(kind, lang)),
            other => {
                flat.push(other);
                continue;
            }
        };
        // Spaces at the edges of italics or bold move outside them.
        let text = wordhord_book::model::plain_text(&inner);
        if text.trim().is_empty() {
            if !text.is_empty() {
                flat.push(Inline::Text(" ".into()));
            }
            continue;
        }
        if text.starts_with(char::is_whitespace) {
            flat.push(Inline::Text(" ".into()));
        }
        let inner = tidy(inner);
        flat.push(match wrap {
            Wrap::Emphasis => Inline::Emphasis(inner),
            Wrap::Strong => Inline::Strong(inner),
            Wrap::Span(kind, lang) => Inline::Span {
                kind,
                lang,
                inlines: inner,
            },
        });
        if text.ends_with(char::is_whitespace) {
            flat.push(Inline::Text(" ".into()));
        }
    }
    let mut merged: Vec<Inline> = Vec::new();
    for i in flat {
        match (merged.last_mut(), i) {
            (Some(Inline::Text(a)), Inline::Text(b)) => a.push_str(&b),
            (_, i) => merged.push(i),
        }
    }
    for i in &mut merged {
        if let Inline::Text(t) = i {
            let lead = t.starts_with(|c: char| c.is_whitespace() && c != '\u{a0}');
            let trail = t.ends_with(|c: char| c.is_whitespace() && c != '\u{a0}');
            let mut s = collapse(t);
            if lead && !s.is_empty() {
                s.insert(0, ' ');
            }
            if trail && !s.is_empty() {
                s.push(' ');
            }
            if s.is_empty() && (lead || trail) {
                s.push(' ');
            }
            *t = s;
        }
    }
    // Trim the ends of the whole run.
    if let Some(Inline::Text(t)) = merged
        .iter_mut()
        .find(|i| matches!(i, Inline::Text(_) | Inline::Emphasis(_) | Inline::Strong(_)))
    {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t)) = merged
        .iter_mut()
        .rev()
        .find(|i| matches!(i, Inline::Text(_) | Inline::Emphasis(_) | Inline::Strong(_)))
    {
        *t = t.trim_end().to_string();
    }
    merged.retain(|i| !matches!(i, Inline::Text(t) if t.is_empty()));
    merged
}

/// The first element matching `selector` at the start of `node`, and the
/// fragment of the link in it: `<i><a href="…#p1.4.prose">Prose</a></i>`.
fn target_link(node: Node, selector: Option<&Selector>) -> Option<(NodeId, String)> {
    let sel = selector?;
    let label = node
        .children()
        .find(|n| n.is_element() || n.text().is_some_and(|t| !t.trim().is_empty()))?;
    if !sel.matches(label) {
        return None;
    }
    let href = label.descendants().find_map(|a| a.attribute("href"))?;
    let (_, fragment) = href.split_once('#')?;
    Some((label.id(), fragment.to_string()))
}

/// The first number in a label: digits, or a Roman numeral ("Canto IV").
fn first_number(s: &str) -> Option<u32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if let Ok(n) = digits.parse() {
        return Some(n);
    }
    s.split(|c: char| !c.is_ascii_alphabetic()).find_map(roman)
}

/// `XXXIV` → 34. Only Roman letters: "I" counts, other words don't.
fn roman(word: &str) -> Option<u32> {
    if word.is_empty() || !word.chars().all(|c| "IVXLCDM".contains(c)) {
        return None;
    }
    let value = |c| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        _ => 1000,
    };
    let digits: Vec<i64> = word.chars().map(value).collect();
    // A smaller digit before a larger one is subtracted: IV, XC.
    let sum: i64 = digits
        .iter()
        .enumerate()
        .map(|(k, &d)| {
            if digits.get(k + 1).is_some_and(|&next| next > d) {
                -d
            } else {
                d
            }
        })
        .sum();
    u32::try_from(sum).ok().filter(|&n| n > 0)
}

/// `5.` → `5`; `5–6.` → `5-6`.
fn number_range(s: &str) -> Option<String> {
    let re = Regex::new(r"(\d+)(?:\s*[–-]\s*(\d+))?").expect("valid");
    let c = re.captures(s)?;
    Some(match c.get(2) {
        Some(b) => format!("{}-{}", &c[1], b.as_str()),
        None => c[1].to_string(),
    })
}

fn sample(node: Node) -> String {
    collapse(
        &node
            .descendants()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect::<String>(),
    )
    .chars()
    .take(60)
    .collect()
}
