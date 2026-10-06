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
    #[serde(rename = "role", default)]
    pub roles: Vec<Role>,
    /// Classes on lines that mean indent steps.
    #[serde(default)]
    pub indent: BTreeMap<String, u8>,
    /// ` | ` in a line is a caesura.
    #[serde(default)]
    pub caesura: bool,
    /// Classes that are only styling: not reported as unmapped.
    #[serde(default)]
    pub styling: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub prefix: String,
    /// The heading text that starts it, matched after collapsing spaces.
    pub start: String,
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
    Drop,
}

/// One step of a selector: a tag, classes and attribute tests.
#[derive(Debug, Clone, PartialEq)]
pub struct Simple {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<(String, bool, String)>,
}

/// `div.endNotes p`: steps from ancestor to the element itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector(pub Vec<Simple>);

const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

impl Selector {
    pub fn parse(s: &str) -> Result<Selector, String> {
        let mut steps = Vec::new();
        for part in s.split_whitespace() {
            let mut simple = Simple {
                tag: None,
                classes: Vec::new(),
                attrs: Vec::new(),
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
        if steps.is_empty() {
            return Err("an empty selector".into());
        }
        Ok(Selector(steps))
    }

    pub fn matches(&self, node: roxmltree::Node) -> bool {
        let (last, ancestors) = self.0.split_last().expect("selectors aren't empty");
        if !matches_simple(last, node) {
            return false;
        }
        let mut wanted = ancestors.iter().rev().peekable();
        let mut up = node.parent_element();
        while let (Some(s), Some(n)) = (wanted.peek(), up) {
            if matches_simple(s, n) {
                wanted.next();
            }
            up = n.parent_element();
        }
        wanted.peek().is_none()
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
    }
}
