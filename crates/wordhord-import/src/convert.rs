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
}

struct CompiledRole<'r> {
    selector: Selector,
    role: &'r Role,
    pattern: Option<Regex>,
    number: Option<Selector>,
}

pub struct Converter<'r> {
    recipe: &'r Recipe,
    roles: Vec<CompiledRole<'r>>,
    pub chapters: Vec<Chapter>,
    pub report: Report,
    active: bool,
    done: bool,
    /// A source page that started between blocks: it goes on the next one.
    pending_page: Vec<Inline>,
    /// Lines outside any stanza element, gathered into the open group.
    loose_open: bool,
}

const CONTAINERS: &[&str] = &[
    "body", "div", "section", "article", "header", "hgroup", "main", "aside", "footer",
];

impl<'r> Converter<'r> {
    pub fn new(recipe: &'r Recipe) -> Result<Self, String> {
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
            roles.push(CompiledRole {
                selector: Selector::parse(&role.select)?,
                role,
                pattern,
                number,
            });
        }
        Ok(Converter {
            recipe,
            roles,
            chapters: Vec::new(),
            report: Report::default(),
            active: false,
            done: false,
            pending_page: Vec::new(),
            loose_open: false,
        })
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    fn role(&self, node: Node) -> Option<&CompiledRole<'r>> {
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
    fn unmapped(&mut self, node: Node) {
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

    /// Walks a document's body.
    pub fn document(&mut self, doc: &roxmltree::Document) {
        if let Some(body) = doc.descendants().find(|n| n.has_tag_name("body")) {
            self.blocks(body);
        }
    }

    fn blocks(&mut self, node: Node) {
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
                _ if !self.active => {}
                Some(Kind::Paragraph) => self.paragraph(child),
                Some(Kind::Stanza) => self.stanza(child),
                Some(Kind::Verse) => self.quoted_verse(child),
                Some(Kind::Line) => self.loose_line(child),
                Some(Kind::Commentary) => self.commentary(child),
                Some(Kind::Page) => {
                    if let Some(p) = self.page(child) {
                        self.pending_page.push(p);
                    }
                }
                None => Self::note(&mut self.report.unhandled, format!("<{tag}>"), child),
            }
        }
    }

    fn heading(&mut self, node: Node) {
        let text = collapse(
            &node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>(),
        );
        if self.recipe.end.as_deref() == Some(text.as_str()) && self.active {
            self.done = true;
            return;
        }
        if let Some(w) = self.recipe.works.iter().find(|w| w.start == text) {
            self.chapters.push(Chapter {
                prefix: w.prefix.clone(),
                blocks: Vec::new(),
            });
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

    fn paragraph(&mut self, node: Node) {
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
        self.push(Out::Paragraph(all));
    }

    fn number(&self, node: Node, selector: Option<&Selector>) -> (Option<String>, Option<NodeId>) {
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

    fn stanza(&mut self, node: Node) {
        let role = self.role(node).expect("a stanza has a role");
        let selector = role.number.clone();
        let (text, skip) = self.number(node, selector.as_ref());
        let number = text.as_deref().and_then(first_number);
        if selector.is_some() && number.is_none() {
            self.report
                .problems
                .push(format!("a stanza without a number: {}", sample(node)));
        }
        let lines = self.lines_of(node, skip);
        if lines.is_empty() {
            self.report.problems.push(format!(
                "a stanza with no lines the recipe recognises: {}",
                sample(node)
            ));
            return;
        }
        self.push(Out::Stanza { number, lines });
    }

    /// Lines with no stanza number: verse quoted in a note goes into the
    /// note; elsewhere it's a verse block of its own.
    fn quoted_verse(&mut self, node: Node) {
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
    fn lines_of(&mut self, node: Node, skip: Option<NodeId>) -> Vec<Line> {
        let line_nodes: Vec<Node> = node
            .descendants()
            .filter(|n| n.id() != node.id())
            .filter(|n| self.role(*n).is_some_and(|r| r.role.kind == Kind::Line))
            .filter(|n| {
                !n.ancestors()
                    .skip(1)
                    .take_while(|a| a.id() != node.id())
                    .any(|a| self.role(a).is_some_and(|r| r.role.kind == Kind::Line))
            })
            .collect();
        let mut lines = Vec::new();
        for (k, l) in line_nodes.into_iter().enumerate() {
            let mut inlines = if k == 0 {
                std::mem::take(&mut self.pending_page)
            } else {
                Vec::new()
            };
            inlines.extend(self.inlines(l, skip));
            lines.push(Line {
                indent: self.indent(l),
                inlines: self.verse_inlines(inlines),
            });
        }
        lines
    }

    fn loose_line(&mut self, node: Node) {
        let mut inlines = std::mem::take(&mut self.pending_page);
        inlines.extend(self.inlines(node, None));
        let line = Line {
            indent: self.indent(node),
            inlines: self.verse_inlines(inlines),
        };
        let open = self.loose_open;
        match self.chapters.last_mut().and_then(|c| c.blocks.last_mut()) {
            Some(Out::Stanza {
                number: None,
                lines,
            }) if open => lines.push(line),
            _ => {
                self.push(Out::Stanza {
                    number: None,
                    lines: vec![line],
                });
                self.loose_open = true;
            }
        }
    }

    fn commentary(&mut self, node: Node) {
        let role = self.role(node).expect("a commentary has a role");
        let selector = role.number.clone();
        let (text, skip) = self.number(node, selector.as_ref());
        let mut paragraph = std::mem::take(&mut self.pending_page);
        paragraph.extend(self.inlines(node, skip));
        let paragraph = tidy(paragraph);
        let prefix = self
            .chapters
            .last()
            .map(|c| c.prefix.clone())
            .unwrap_or_default();
        match text.as_deref().and_then(number_range) {
            Some(r) => self.push(Out::Commentary {
                on: format!("{prefix}.{r}"),
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

    fn page(&mut self, node: Node) -> Option<Inline> {
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

    fn indent(&self, node: Node) -> u8 {
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
        if !self.recipe.caesura {
            return inlines;
        }
        // Editions pad the bar with ordinary or non-breaking spaces.
        let bar = Regex::new(r"[\s\u{a0}]+\|[\s\u{a0}]+").expect("valid");
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
    fn inlines(&mut self, node: Node, skip: Option<NodeId>) -> Vec<Inline> {
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
                        out.push(Inline::Emphasis(self.inlines(child, skip)));
                    }
                    "b" | "strong" => {
                        self.unmapped(child);
                        out.push(Inline::Strong(self.inlines(child, skip)));
                    }
                    "br" => out.push(Inline::Text(" ".into())),
                    "a" | "span" | "small" | "abbr" => {
                        self.unmapped(child);
                        out.extend(self.inlines(child, skip));
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

/// Whitespace runs as one space, and trimmed.
pub fn collapse(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() && c != '\u{a0}')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Joins text, collapses whitespace inside it, trims the ends, and drops
/// what's left empty.
pub fn tidy(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut flat: Vec<Inline> = Vec::new();
    for i in inlines {
        let (inner, strong) = match i {
            Inline::Emphasis(c) => (c, false),
            Inline::Strong(c) => (c, true),
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
        flat.push(if strong {
            Inline::Strong(inner)
        } else {
            Inline::Emphasis(inner)
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

fn first_number(s: &str) -> Option<u32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
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
