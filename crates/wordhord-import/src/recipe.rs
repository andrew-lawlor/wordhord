//! An import recipe: what an edition's markup means. Publishers invent
//! their own class names, so the mapping is written down per edition and
//! reviewed, never guessed (SPEC.md §4, Import).

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Written out as the book's `book.toml`.
    pub book: toml::Table,
    /// The works to take, in order, each starting at a heading.
    #[serde(rename = "work")]
    pub works: Vec<Work>,
    /// The heading where the selection ends, if not at the end of the book.
    pub end: Option<String>,
    /// Which headings can start or end a work, when others share their
    /// text (a chapter title, a synopsis subheading): a selector.
    pub starts: Option<String>,
    #[serde(rename = "role", default)]
    pub roles: Vec<Role>,
    /// Classes on lines that mean indent steps.
    #[serde(default)]
    pub indent: BTreeMap<String, u8>,
    /// ` | ` in a line is a caesura.
    #[serde(default)]
    pub caesura: bool,
    /// Or a regex for the caesura, for editions without a bar: three
    /// non-breaking spaces are `'\x{a0}{2,}'`.
    pub caesura_pattern: Option<String>,
    /// Classes that are only styling: not reported as unmapped.
    #[serde(default)]
    pub styling: Vec<String>,
    /// Keep the languages elements name (`lang="it"`) as language spans.
    #[serde(default)]
    pub languages: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub prefix: String,
    /// The heading text that starts it, matched after collapsing spaces.
    /// Works are matched in order, so two can share a title.
    pub start: String,
    /// The heading text where it stops, if something else follows it in
    /// the same chapter (a translation, an apparatus).
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// A selector: `div.lgouter`, `p.line`, `div.endNotes p`,
    /// `span[epub:type=pagebreak]`, `a[id^=page_]`.
    pub select: String,
    #[serde(rename = "as")]
    pub kind: Kind,
    /// For headings.
    pub level: Option<u16>,
    /// For stanzas and commentary: the child holding the number.
    pub number: Option<String>,
    /// For commentary without a number: the element at its start that
    /// links to its subject, such as a passage of prose.
    pub target: Option<String>,
    /// For pages: a regex whose first group is the page number, matched
    /// against `from` (the text by default). No match: dropped.
    pub pattern: Option<String>,
    /// For pages: `text`, or an attribute name such as `id` or `title`.
    pub from: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Heading,
    Paragraph,
    Stanza,
    /// Lines with no stanza number: verse quoted in a note goes into it.
    Verse,
    Line,
    Page,
    Commentary,
    /// A block that only holds other blocks.
    Container,
    /// An inline element whose text stays, without meaning.
    Unwrap,
    /// Text lost from the source, marked by the edition (rows of
    /// asterisks).
    Gap,
    /// A thematic break between sections: a rule, or a row of dots.
    Break,
    /// Inline: emphasis, usually italic.
    Emphasis,
    /// Inline: strong emphasis, usually bold.
    Strong,
    /// Inline: the editor's emendation of the manuscript (TEI `<corr>`).
    Corr,
    /// Inline: a footnote. Its paragraphs become a definition at the end
    /// of the chapter; a reference stays where it was. Labelled by its
    /// `n` attribute, or counted.
    Note,
    /// Inline: a link to a note elsewhere (an endnote). The note it links
    /// to becomes a footnote here, labelled by the link's text.
    NoteRef,
    /// A poem, or a canto or book of one: groups of lines (`<p>`), after
    /// its heading. Numbered with `number`, its lines are counted.
    Poem,
    /// A block quotation.
    Quote,
    /// A table.
    Table,
    /// A drama table: each row a speech.
    Drama,
    /// The speaker's cell in a drama row.
    Speaker,
    Drop,
}

/// One step of a selector: a tag, classes and attribute tests.
#[derive(Debug, Clone, PartialEq)]
pub struct Simple {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, bool, String)>,
    /// The step before this one must be its parent (`>`), not just an
    /// ancestor.
    pub child: bool,
}

/// `div.endNotes p`, `div.volume > h2`: steps from ancestor to the element
/// itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector(pub Vec<Simple>);

const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

impl Selector {
    pub fn parse(s: &str) -> Result<Selector, String> {
        let mut steps = Vec::new();
        let mut child = false;
        for part in s.split_whitespace() {
            if part == ">" {
                if steps.is_empty() || child {
                    return Err(format!("`{s}`: `>` needs a step on each side"));
                }
                child = true;
                continue;
            }
            let mut simple = Simple {
                tag: None,
                classes: Vec::new(),
                attrs: Vec::new(),
                child: std::mem::take(&mut child),
            };
            let (head, attrs) = part.split_once('[').map_or((part, ""), |(h, a)| (h, a));
            let mut pieces = head.split('.');
            let tag = pieces.next().unwrap_or_default();
            if !tag.is_empty() {
                simple.tag = Some(tag.to_string());
            }
            simple.classes = pieces.map(String::from).collect();
            if !attrs.is_empty() {
                for a in format!("[{attrs}").split('[').filter(|a| !a.is_empty()) {
                    let a = a
                        .strip_suffix(']')
                        .ok_or_else(|| format!("`{s}`: unclosed ["))?;
                    let (name, value, prefix) = if let Some((n, v)) = a.split_once("^=") {
                        (n, v, true)
                    } else if let Some((n, v)) = a.split_once('=') {
                        (n, v, false)
                    } else {
                        return Err(format!(
                            "`{s}`: attribute tests are [name=value] or [name^=value]"
                        ));
                    };
                    simple.attrs.push((
                        name.to_string(),
                        prefix,
                        value.trim_matches('"').to_string(),
                    ));
                }
            }
            steps.push(simple);
        }
        if steps.is_empty() || child {
            return Err(format!("`{s}`: an empty or unfinished selector"));
        }
        Ok(Selector(steps))
    }

    pub fn matches(&self, node: roxmltree::Node) -> bool {
        matches_from(&self.0, node)
    }
}

/// Whether the last step matches `node`, and the steps before it match its
/// parent or some ancestor, as each step's `child` says.
fn matches_from(steps: &[Simple], node: roxmltree::Node) -> bool {
    let Some((last, before)) = steps.split_last() else {
        return true;
    };
    if !matches_simple(last, node) {
        return false;
    }
    if before.is_empty() {
        return true;
    }
    if last.child {
        node.parent_element()
            .is_some_and(|p| matches_from(before, p))
    } else {
        node.ancestors()
            .skip(1)
            .filter(|a| a.is_element())
            .any(|a| matches_from(before, a))
    }
}

fn matches_simple(s: &Simple, node: roxmltree::Node) -> bool {
    if !node.is_element() {
        return false;
    }
    if let Some(t) = &s.tag {
        if node.tag_name().name() != t {
            return false;
        }
    }
    let classes: Vec<&str> = node
        .attribute("class")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if !s.classes.iter().all(|c| classes.contains(&c.as_str())) {
        return false;
    }
    s.attrs.iter().all(|(name, prefix, value)| {
        let v = attribute(node, name);
        match v {
            Some(v) if *prefix => v.starts_with(value.as_str()),
            Some(v) => v.split_whitespace().any(|w| w == value) || v == value,
            None => false,
        }
    })
}

/// An attribute, with `epub:` meaning the EPUB namespace.
pub fn attribute<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    match name.strip_prefix("epub:") {
        Some(local) => node.attribute((EPUB_NS, local)),
        None => node.attribute(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_match_tags_classes_attributes_and_ancestors() {
        let xml = r#"<body xmlns:epub="http://www.idpf.org/2007/ops"><div class="div2 endNotes"><p class="first" id="n1">x</p></div><p class="line xd1">y</p><span epub:type="pagebreak" id="page5"/></body>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let find = |id: &str| {
            doc.descendants()
                .find(|n| n.attribute("id") == Some(id))
                .unwrap()
        };
        let note = find("n1");
        assert!(Selector::parse("div.endNotes p").unwrap().matches(note));
        assert!(Selector::parse("p.first").unwrap().matches(note));
        assert!(!Selector::parse("p.line").unwrap().matches(note));
        let line = doc
            .descendants()
            .find(|n| n.attribute("class") == Some("line xd1"))
            .unwrap();
        assert!(Selector::parse("p.line").unwrap().matches(line));
        assert!(!Selector::parse("div.endNotes p").unwrap().matches(line));
        let page = find("page5");
        assert!(Selector::parse("span[epub:type=pagebreak]")
            .unwrap()
            .matches(page));
        assert!(Selector::parse("[id^=page]").unwrap().matches(page));
        assert!(Selector::parse("body > p.line").unwrap().matches(line));
        assert!(!Selector::parse("body > p.first").unwrap().matches(note));
        assert!(Selector::parse("body > div p.first").unwrap().matches(note));
        assert!(Selector::parse("div >").is_err());
    }
}
