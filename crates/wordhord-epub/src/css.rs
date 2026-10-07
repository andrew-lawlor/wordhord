//! A small CSS engine: enough to know what an element means.
//!
//! Real EPUBs keep their meaning in classes. A paragraph is
//! `<div class="p-indent">`, italics are `<span class="i">`, a verse line is
//! `<p class="pty">`. Only the stylesheet says so. This reads selectors
//! (tags, classes, IDs, attributes, descendants, children, siblings,
//! `:first-child`), weighs them by specificity, cascades and inherits, and
//! computes the dozen properties that carry meaning. Layout stays
//! Wordhord's: sizes, colours, fonts and floats are not its business.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Inline,
    Block,
    ListItem,
    Table,
    TableRow,
    TableCell,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vertical {
    #[default]
    Baseline,
    Super,
    Sub,
}

/// A length: ems of the text (px over 16, pt over 12), plus a share of the
/// line's width for percentages.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Length {
    pub em: f32,
    pub percent: f32,
}

impl Length {
    pub fn is_zero(&self) -> bool {
        self.em.abs() < 0.01 && self.percent.abs() < 0.01
    }
}

/// What an element means, as far as Wordhord cares.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub display: Display,
    pub italic: bool,
    pub bold: bool,
    pub small_caps: bool,
    pub monospace: bool,
    pub uppercase: bool,
    pub align: Align,
    pub text_indent: Length,
    pub margin_left: Length,
    pub padding_left: Length,
    pub margin_top: Length,
    pub margin_bottom: Length,
    pub vertical: Vertical,
    /// `white-space: pre`: keep spaces and line breaks.
    pub pre: bool,
    pub break_before: bool,
    /// Size against the body text: 1.0 is body size.
    pub size: f32,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            display: Display::Inline,
            italic: false,
            bold: false,
            small_caps: false,
            monospace: false,
            uppercase: false,
            align: Align::Start,
            text_indent: Length::default(),
            margin_left: Length::default(),
            padding_left: Length::default(),
            margin_top: Length::default(),
            margin_bottom: Length::default(),
            vertical: Vertical::Baseline,
            pre: false,
            break_before: false,
            size: 1.0,
        }
    }
}

impl Style {
    /// A child's starting point: the inherited properties of its parent,
    /// the rest at their defaults.
    fn inherit(parent: &Style) -> Style {
        Style {
            italic: parent.italic,
            bold: parent.bold,
            small_caps: parent.small_caps,
            monospace: parent.monospace,
            uppercase: parent.uppercase,
            align: parent.align,
            text_indent: parent.text_indent,
            pre: parent.pre,
            size: parent.size,
            ..Style::default()
        }
    }

    /// How far the block is set in from its parent's left edge.
    pub fn left(&self) -> Length {
        Length {
            em: self.margin_left.em + self.padding_left.em,
            percent: self.margin_left.percent + self.padding_left.percent,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Decl {
    Display(Display),
    Italic(bool),
    Bold(bool),
    SmallCaps(bool),
    Monospace(bool),
    Uppercase(bool),
    Align(Align),
    TextIndent(Length),
    MarginLeft(Length),
    PaddingLeft(Length),
    MarginTop(Length),
    MarginBottom(Length),
    Vertical(Vertical),
    Pre(bool),
    BreakBefore(bool),
    /// Against the parent (`em`, `%`, `smaller`).
    SizeRelative(f32),
    /// Against the body (`px`, `pt`, `rem`, `large`).
    SizeAbsolute(f32),
}

#[derive(Debug, Clone, PartialEq)]
enum AttrOp {
    Exists,
    Equals(String),
    /// `~=`: one of the space-separated words.
    Includes(String),
    /// `^=`
    Prefix(String),
    /// `$=`
    Suffix(String),
    /// `*=`
    Contains(String),
    /// `|=`: the value, or the value then a hyphen.
    Dash(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, AttrOp)>,
    first_child: bool,
    last_child: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Debug, Clone, PartialEq)]
struct Selector {
    /// Left to right; `combinators[k]` joins `compounds[k]` and `[k + 1]`.
    compounds: Vec<Compound>,
    combinators: Vec<Combinator>,
}

impl Selector {
    fn specificity(&self) -> (u32, u32, u32) {
        let mut s = (0, 0, 0);
        for c in &self.compounds {
            s.0 += c.id.is_some() as u32;
            s.1 += (c.classes.len() + c.attrs.len()) as u32
                + c.first_child as u32
                + c.last_child as u32;
            s.2 += c.tag.is_some() as u32;
        }
        s
    }

    fn subject(&self) -> &Compound {
        self.compounds.last().expect("a selector has a compound")
    }
}

/// A matched declaration: important, specificity, order, and what it says.
type Hit<'a> = (bool, (u32, u32, u32), usize, &'a Decl);

struct Rule {
    selector: Selector,
    specificity: (u32, u32, u32),
    order: usize,
    decls: Vec<(Decl, bool)>,
}

/// Rules from the reader's own defaults and the book's stylesheets.
pub struct Stylesheet {
    rules: Vec<Rule>,
    /// Rules by their subject's ID (`#x`), class (`.x`) or tag; the rest.
    index: HashMap<String, Vec<usize>>,
    universal: Vec<usize>,
    /// Selectors skipped because they use something this engine doesn't
    /// read (`::before`, `:hover`, `:not()`).
    pub skipped: usize,
}

/// How a browser styles HTML before any stylesheet: the part that carries
/// meaning.
const DEFAULTS: &str = "
    html, body, div, p, section, article, aside, nav, header, footer, main,
    blockquote, figure, figcaption, address, center, hgroup, hr, pre,
    h1, h2, h3, h4, h5, h6, dl, dt, dd, ol, ul, form, fieldset, details, summary,
    caption { display: block }
    li { display: list-item }
    table { display: table }
    tr { display: table-row }
    td, th { display: table-cell }
    head, script, style, title, meta, link, template, noscript { display: none }
    [hidden] { display: none }
    i, em, cite, var, dfn, address { font-style: italic }
    b, strong, th, h1, h2, h3, h4, h5, h6 { font-weight: bold }
    code, kbd, samp, tt, pre { font-family: monospace }
    pre { white-space: pre }
    sup { vertical-align: super }
    sub { vertical-align: sub }
    center { text-align: center }
    blockquote { margin-left: 2em }
    ol, ul { padding-left: 2em }
    dd { margin-left: 2em }
    h1 { font-size: 2em }
    h2 { font-size: 1.5em }
    h3 { font-size: 1.17em }
    h5 { font-size: 0.83em }
    h6 { font-size: 0.67em }
    small { font-size: smaller }
    big { font-size: larger }
";

impl Stylesheet {
    /// The defaults, then each stylesheet in order.
    pub fn new<S: AsRef<str>>(sheets: &[S]) -> Stylesheet {
        let mut sheet = Stylesheet {
            rules: Vec::new(),
            index: HashMap::new(),
            universal: Vec::new(),
            skipped: 0,
        };
        sheet.add(DEFAULTS);
        for s in sheets {
            sheet.add(s.as_ref());
        }
        sheet
    }

    pub fn rules(&self) -> usize {
        self.rules.len()
    }

    fn add(&mut self, css: &str) {
        let css = strip_comments(css);
        let mut out = Vec::new();
        parse_rules(&css, &mut out);
        for (selectors, body) in out {
            let decls = parse_decls(&body);
            if decls.is_empty() {
                continue;
            }
            for sel in selectors.split(',') {
                let Some(selector) = parse_selector(sel.trim()) else {
                    self.skipped += 1;
                    continue;
                };
                let k = self.rules.len();
                let subject = selector.subject();
                let key = if let Some(id) = &subject.id {
                    Some(format!("#{id}"))
                } else if let Some(class) = subject.classes.first() {
                    Some(format!(".{class}"))
                } else {
                    subject.tag.clone()
                };
                match key {
                    Some(key) => self.index.entry(key).or_default().push(k),
                    None => self.universal.push(k),
                }
                self.rules.push(Rule {
                    specificity: selector.specificity(),
                    selector,
                    order: k,
                    decls: decls.clone(),
                });
            }
        }
    }

    /// An element's style, given its parent's.
    pub fn style(&self, node: roxmltree::Node, parent: &Style) -> Style {
        let mut style = Style::inherit(parent);
        let mut candidates: Vec<usize> = self.universal.clone();
        if let Some(id) = node.attribute("id") {
            candidates.extend(self.index.get(&format!("#{id}")).into_iter().flatten());
        }
        for class in node
            .attribute("class")
            .unwrap_or_default()
            .split_whitespace()
        {
            candidates.extend(self.index.get(&format!(".{class}")).into_iter().flatten());
        }
        let tag = node.tag_name().name().to_ascii_lowercase();
        candidates.extend(self.index.get(&tag).into_iter().flatten());
        candidates.sort_unstable();
        candidates.dedup();

        let mut hits: Vec<Hit> = Vec::new();
        for k in candidates {
            let rule = &self.rules[k];
            if matches(&rule.selector, rule.selector.compounds.len() - 1, node) {
                for (decl, important) in &rule.decls {
                    hits.push((*important, rule.specificity, rule.order, decl));
                }
            }
        }
        // An inline `style` beats every rule but an important one.
        let inline = node.attribute("style").map(parse_decls).unwrap_or_default();
        for (decl, important) in &inline {
            hits.push((*important, (1000, 0, 0), usize::MAX, decl));
        }
        hits.sort_by_key(|h| (h.0, h.1, h.2));
        for (_, _, _, decl) in hits {
            apply(&mut style, decl, parent);
        }
        style
    }
}

fn apply(s: &mut Style, decl: &Decl, parent: &Style) {
    match decl {
        Decl::Display(d) => s.display = *d,
        Decl::Italic(b) => s.italic = *b,
        Decl::Bold(b) => s.bold = *b,
        Decl::SmallCaps(b) => s.small_caps = *b,
        Decl::Monospace(b) => s.monospace = *b,
        Decl::Uppercase(b) => s.uppercase = *b,
        Decl::Align(a) => s.align = *a,
        Decl::TextIndent(l) => s.text_indent = *l,
        Decl::MarginLeft(l) => s.margin_left = *l,
        Decl::PaddingLeft(l) => s.padding_left = *l,
        Decl::MarginTop(l) => s.margin_top = *l,
        Decl::MarginBottom(l) => s.margin_bottom = *l,
        Decl::Vertical(v) => s.vertical = *v,
        Decl::Pre(b) => s.pre = *b,
        Decl::BreakBefore(b) => s.break_before = *b,
        Decl::SizeRelative(f) => s.size = parent.size * f,
        Decl::SizeAbsolute(f) => s.size = *f,
    }
}

// ---- matching ------------------------------------------------------

fn matches(sel: &Selector, k: usize, node: roxmltree::Node) -> bool {
    if !compound_matches(&sel.compounds[k], node) {
        return false;
    }
    if k == 0 {
        return true;
    }
    match sel.combinators[k - 1] {
        Combinator::Child => node
            .parent()
            .filter(|p| p.is_element())
            .is_some_and(|p| matches(sel, k - 1, p)),
        Combinator::Descendant => node
            .ancestors()
            .skip(1)
            .filter(|a| a.is_element())
            .any(|a| matches(sel, k - 1, a)),
        Combinator::Adjacent => previous_element(node).is_some_and(|p| matches(sel, k - 1, p)),
        Combinator::Sibling => {
            let mut p = previous_element(node);
            while let Some(n) = p {
                if matches(sel, k - 1, n) {
                    return true;
                }
                p = previous_element(n);
            }
            false
        }
    }
}

fn previous_element<'a, 'b>(node: roxmltree::Node<'a, 'b>) -> Option<roxmltree::Node<'a, 'b>> {
    let mut p = node.prev_sibling();
    while let Some(n) = p {
        if n.is_element() {
            return Some(n);
        }
        p = n.prev_sibling();
    }
    None
}

fn compound_matches(c: &Compound, node: roxmltree::Node) -> bool {
    if !node.is_element() {
        return false;
    }
    if let Some(tag) = &c.tag {
        if !node.tag_name().name().eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if node.attribute("id") != Some(id.as_str()) {
            return false;
        }
    }
    if !c.classes.is_empty() {
        let classes: Vec<&str> = node
            .attribute("class")
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        if !c.classes.iter().all(|k| classes.contains(&k.as_str())) {
            return false;
        }
    }
    for (name, op) in &c.attrs {
        // `epub|type` and `epub:type` both match the attribute named `type`.
        let value = node
            .attributes()
            .find(|a| a.name().eq_ignore_ascii_case(name))
            .map(|a| a.value());
        let ok = match (op, value) {
            (_, None) => false,
            (AttrOp::Exists, Some(_)) => true,
            (AttrOp::Equals(v), Some(x)) => x == v,
            (AttrOp::Includes(v), Some(x)) => x.split_whitespace().any(|w| w == v),
            (AttrOp::Prefix(v), Some(x)) => x.starts_with(v.as_str()),
            (AttrOp::Suffix(v), Some(x)) => x.ends_with(v.as_str()),
            (AttrOp::Contains(v), Some(x)) => x.contains(v.as_str()),
            (AttrOp::Dash(v), Some(x)) => x == v || x.starts_with(&format!("{v}-")),
        };
        if !ok {
            return false;
        }
    }
    if c.first_child && previous_element(node).is_some() {
        return false;
    }
    if c.last_child {
        let mut n = node.next_sibling();
        while let Some(s) = n {
            if s.is_element() {
                return false;
            }
            n = s.next_sibling();
        }
    }
    true
}

// ---- parsing -------------------------------------------------------

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `selectors { body }` pairs, descending into `@media` blocks meant for
/// screens like ours and skipping every other at-rule.
fn parse_rules(css: &str, out: &mut Vec<(String, String)>) {
    let mut rest = css.trim_start();
    while !rest.is_empty() {
        if let Some(at) = rest.strip_prefix('@') {
            let brace = at.find('{');
            let semi = at.find(';');
            match (brace, semi) {
                // `@import …;`, `@charset …;`, `@namespace …;`
                (b, Some(s)) if b.is_none_or(|b| s < b) => {
                    rest = at[s + 1..].trim_start();
                }
                (Some(b), _) => {
                    let prelude = at[..b].trim().to_ascii_lowercase();
                    let Some(end) = block_end(&at[b..]) else {
                        return;
                    };
                    let body = &at[b + 1..b + end];
                    if prelude.starts_with("media") && wanted_media(&prelude) {
                        parse_rules(body, out);
                    }
                    rest = at[b + end + 1..].trim_start();
                }
                _ => return,
            }
            continue;
        }
        let Some(b) = rest.find('{') else { return };
        let Some(end) = block_end(&rest[b..]) else {
            return;
        };
        let selectors = rest[..b].trim().to_string();
        let body = rest[b + 1..b + end].to_string();
        if !selectors.is_empty() {
            out.push((selectors, body));
        }
        rest = rest[b + end + 1..].trim_start();
    }
}

/// Media queries for Kindles are not for us; `not amzn-…` ones are.
fn wanted_media(prelude: &str) -> bool {
    let query = prelude.trim_start_matches("media").trim();
    if query.contains("amzn") {
        return query.contains("not");
    }
    !query.contains("print") || query.contains("screen")
}

/// Where the block that starts at `s[0] == '{'` closes, counting nesting.
fn block_end(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (k, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() {
        return None;
    }
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut k = 0;
    let mut current = Compound::default();
    let mut started = false;
    let mut pending: Option<Combinator> = None;
    let ident = |k: &mut usize| -> String {
        let start = *k;
        while *k < chars.len()
            && (chars[*k].is_alphanumeric() || matches!(chars[*k], '-' | '_' | '\\'))
        {
            *k += 1;
        }
        chars[start..*k]
            .iter()
            .collect::<String>()
            .replace('\\', "")
    };
    while k < chars.len() {
        let c = chars[k];
        if c.is_whitespace() || matches!(c, '>' | '+' | '~') {
            let mut comb = Combinator::Descendant;
            while k < chars.len()
                && (chars[k].is_whitespace() || matches!(chars[k], '>' | '+' | '~'))
            {
                match chars[k] {
                    '>' => comb = Combinator::Child,
                    '+' => comb = Combinator::Adjacent,
                    '~' => comb = Combinator::Sibling,
                    _ => {}
                }
                k += 1;
            }
            if started && k < chars.len() {
                compounds.push(std::mem::take(&mut current));
                pending = Some(comb);
                started = false;
            }
            continue;
        }
        if let Some(comb) = pending.take() {
            combinators.push(comb);
        }
        started = true;
        match c {
            '*' => k += 1,
            '.' => {
                k += 1;
                current.classes.push(ident(&mut k));
            }
            '#' => {
                k += 1;
                current.id = Some(ident(&mut k));
            }
            '[' => {
                let end = chars[k..].iter().position(|&c| c == ']')? + k;
                let inner: String = chars[k + 1..end].iter().collect();
                current.attrs.push(parse_attr(&inner)?);
                k = end + 1;
            }
            ':' => {
                k += 1;
                if chars.get(k) == Some(&':') {
                    return None; // ::before, ::first-line
                }
                match ident(&mut k).as_str() {
                    "first-child" => current.first_child = true,
                    "last-child" => current.last_child = true,
                    // Links look the same visited or not.
                    "link" | "visited" => {}
                    _ => return None,
                }
            }
            c if c.is_alphabetic() => {
                let mut name = ident(&mut k);
                // `epub|type` style namespaces on tags.
                if chars.get(k) == Some(&'|') {
                    k += 1;
                    name = ident(&mut k);
                }
                current.tag = Some(name.to_ascii_lowercase());
            }
            _ => return None,
        }
    }
    if !started {
        return None;
    }
    compounds.push(current);
    if combinators.len() + 1 != compounds.len() {
        return None;
    }
    Some(Selector {
        compounds,
        combinators,
    })
}

fn parse_attr(inner: &str) -> Option<(String, AttrOp)> {
    let ops = ["~=", "^=", "$=", "*=", "|=", "="];
    for op in ops {
        if let Some((name, value)) = inner.split_once(op) {
            let name = attr_name(name);
            let value = value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
            let op = match op {
                "~=" => AttrOp::Includes(value),
                "^=" => AttrOp::Prefix(value),
                "$=" => AttrOp::Suffix(value),
                "*=" => AttrOp::Contains(value),
                "|=" => AttrOp::Dash(value),
                _ => AttrOp::Equals(value),
            };
            return Some((name, op));
        }
    }
    Some((attr_name(inner), AttrOp::Exists))
}

/// `epub|type` and `epub\:type` name the attribute `type`.
fn attr_name(name: &str) -> String {
    let name = name.trim();
    let name = name.rsplit(['|', ':']).next().unwrap_or(name);
    name.trim_end_matches('\\').to_ascii_lowercase()
}

fn parse_decls(body: &str) -> Vec<(Decl, bool)> {
    let mut out = Vec::new();
    for part in body.split(';') {
        let Some((prop, value)) = part.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let mut value = value.trim().to_ascii_lowercase();
        let important = value.ends_with("!important");
        if important {
            value = value.trim_end_matches("!important").trim().to_string();
        }
        for decl in decls_for(&prop, &value) {
            out.push((decl, important));
        }
    }
    out
}

fn decls_for(prop: &str, v: &str) -> Vec<Decl> {
    let one = |d: Option<Decl>| d.into_iter().collect();
    match prop {
        "display" => one(match v {
            "none" => Some(Decl::Display(Display::None)),
            "block" | "flex" | "grid" | "flow-root" => Some(Decl::Display(Display::Block)),
            "inline" | "inline-block" | "inline-flex" => Some(Decl::Display(Display::Inline)),
            "list-item" => Some(Decl::Display(Display::ListItem)),
            "table" => Some(Decl::Display(Display::Table)),
            "table-row" => Some(Decl::Display(Display::TableRow)),
            "table-cell" => Some(Decl::Display(Display::TableCell)),
            _ => None,
        }),
        "visibility" if v == "hidden" => vec![Decl::Display(Display::None)],
        "font-style" => one(match v {
            "italic" | "oblique" => Some(Decl::Italic(true)),
            "normal" => Some(Decl::Italic(false)),
            _ => None,
        }),
        "font-weight" => one(weight(v).map(Decl::Bold)),
        "font-variant" | "font-variant-caps" => one(match v {
            v if v.contains("small-caps") => Some(Decl::SmallCaps(true)),
            "normal" | "none" => Some(Decl::SmallCaps(false)),
            _ => None,
        }),
        "font-family" => vec![Decl::Monospace(is_monospace(v))],
        "font" => {
            // The shorthand: pick out what it says about style and weight.
            let mut out = Vec::new();
            for word in v.split_whitespace() {
                match word {
                    "italic" | "oblique" => out.push(Decl::Italic(true)),
                    "small-caps" => out.push(Decl::SmallCaps(true)),
                    w => {
                        if let Some(b) = weight(w).filter(|b| *b) {
                            out.push(Decl::Bold(b));
                        }
                    }
                }
            }
            if is_monospace(v) {
                out.push(Decl::Monospace(true));
            }
            out
        }
        "text-transform" => one(match v {
            "uppercase" => Some(Decl::Uppercase(true)),
            "none" => Some(Decl::Uppercase(false)),
            _ => None,
        }),
        "text-align" => one(match v {
            "center" => Some(Decl::Align(Align::Center)),
            "right" | "end" => Some(Decl::Align(Align::End)),
            "justify" => Some(Decl::Align(Align::Justify)),
            "left" | "start" => Some(Decl::Align(Align::Start)),
            _ => None,
        }),
        "text-indent" => one(length(v).map(Decl::TextIndent)),
        "margin-left" => one(length(v).map(Decl::MarginLeft)),
        "padding-left" => one(length(v).map(Decl::PaddingLeft)),
        "margin-top" => one(length(v).map(Decl::MarginTop)),
        "margin-bottom" => one(length(v).map(Decl::MarginBottom)),
        "margin" => {
            let parts: Vec<Option<Length>> = v.split_whitespace().map(length).collect();
            let (top, bottom, left) = match parts.as_slice() {
                [a] => (*a, *a, *a),
                [a, b] => (*a, *a, *b),
                [a, b, c] => (*a, *c, *b),
                [a, _, c, d] => (*a, *c, *d),
                _ => (None, None, None),
            };
            let mut out = Vec::new();
            out.extend(top.map(Decl::MarginTop));
            out.extend(bottom.map(Decl::MarginBottom));
            out.extend(left.map(Decl::MarginLeft));
            out
        }
        "padding" => {
            let parts: Vec<Option<Length>> = v.split_whitespace().map(length).collect();
            // One value is all sides; two or three put left second; four,
            // fourth.
            let left = match parts.as_slice() {
                [a] => *a,
                [_, b] | [_, b, _] => *b,
                [_, _, _, d] => *d,
                _ => None,
            };
            one(left.map(Decl::PaddingLeft))
        }
        "vertical-align" => one(match v {
            "super" | "top" | "text-top" => Some(Decl::Vertical(Vertical::Super)),
            "sub" | "bottom" | "text-bottom" => Some(Decl::Vertical(Vertical::Sub)),
            "baseline" => Some(Decl::Vertical(Vertical::Baseline)),
            _ => None,
        }),
        "white-space" => one(match v {
            "pre" | "pre-wrap" | "pre-line" | "break-spaces" => Some(Decl::Pre(true)),
            "normal" | "nowrap" => Some(Decl::Pre(false)),
            _ => None,
        }),
        "page-break-before" | "break-before" => one(match v {
            "always" | "page" | "left" | "right" | "recto" | "verso" => {
                Some(Decl::BreakBefore(true))
            }
            "auto" | "avoid" => Some(Decl::BreakBefore(false)),
            _ => None,
        }),
        "font-size" => one(size(v)),
        _ => Vec::new(),
    }
}

fn weight(v: &str) -> Option<bool> {
    match v {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        n => n.parse::<u32>().ok().map(|n| n >= 600),
    }
}

fn is_monospace(v: &str) -> bool {
    [
        "monospace",
        "mono",
        "courier",
        "consolas",
        "menlo",
        "monaco",
    ]
    .iter()
    .any(|m| v.contains(m))
}

/// A length in ems. `auto` and unknown units are nothing.
fn length(v: &str) -> Option<Length> {
    let v = v.trim();
    if v == "0" {
        return Some(Length::default());
    }
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
        .unwrap_or(v.len());
    let n: f32 = v[..split].parse().ok()?;
    let em = match &v[split..] {
        "em" | "rem" => n,
        "ex" | "ch" => n * 0.5,
        "px" => n / 16.0,
        "pt" => n / 12.0,
        "pc" => n,
        "in" => n * 6.0,
        "cm" => n * 2.36,
        "mm" => n * 0.236,
        "%" => {
            return Some(Length {
                em: 0.0,
                percent: n / 100.0,
            })
        }
        _ => return None,
    };
    Some(Length { em, percent: 0.0 })
}

fn size(v: &str) -> Option<Decl> {
    let keyword = |f: f32| Some(Decl::SizeAbsolute(f));
    match v {
        "xx-small" => keyword(0.6),
        "x-small" => keyword(0.75),
        "small" => keyword(0.89),
        "medium" => keyword(1.0),
        "large" => keyword(1.2),
        "x-large" => keyword(1.5),
        "xx-large" => keyword(2.0),
        "smaller" => Some(Decl::SizeRelative(0.83)),
        "larger" => Some(Decl::SizeRelative(1.2)),
        v if v.ends_with("rem") => v
            .trim_end_matches("rem")
            .parse()
            .ok()
            .map(Decl::SizeAbsolute),
        v if v.ends_with("em") => v
            .trim_end_matches("em")
            .parse()
            .ok()
            .map(Decl::SizeRelative),
        v if v.ends_with('%') => v
            .trim_end_matches('%')
            .parse::<f32>()
            .ok()
            .map(|n| Decl::SizeRelative(n / 100.0)),
        v if v.ends_with("px") => v
            .trim_end_matches("px")
            .parse::<f32>()
            .ok()
            .map(|n| Decl::SizeAbsolute(n / 16.0)),
        v if v.ends_with("pt") => v
            .trim_end_matches("pt")
            .parse::<f32>()
            .ok()
            .map(|n| Decl::SizeAbsolute(n / 12.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The style of the first element with `id`, cascading from the root.
    fn style_of(css: &str, html: &str, id: &str) -> Style {
        let doc = crate::xml::parse(html).unwrap();
        let sheet = Stylesheet::new(&[css]);
        let target = doc
            .descendants()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap();
        let mut chain: Vec<roxmltree::Node> =
            target.ancestors().filter(|n| n.is_element()).collect();
        chain.reverse();
        let mut style = Style::default();
        for n in chain {
            style = sheet.style(n, &style);
        }
        style
    }

    #[test]
    fn classes_carry_meaning() {
        // The Hammer and the Cross: paragraphs are divs, italics are spans.
        let css = "div.p-indent { text-indent: 1.2em; margin: 0 } span.i { font-style: italic }";
        let html = r#"<body><div class="p-indent" id="p">He saw <span class="i" id="s">Ragnar</span>.</div></body>"#;
        assert_eq!(style_of(css, html, "p").text_indent.em, 1.2);
        assert_eq!(style_of(css, html, "p").display, Display::Block);
        assert!(style_of(css, html, "s").italic);
        assert!(!style_of(css, html, "p").italic);
    }

    #[test]
    fn specificity_order_and_importance() {
        let css = "p { font-style: italic } .plain { font-style: normal } p { font-weight: bold !important } #x { font-weight: normal }";
        let html = r#"<body><p class="plain" id="x">A</p></body>"#;
        let s = style_of(css, html, "x");
        assert!(!s.italic, "a class beats a tag");
        assert!(s.bold, "important beats an ID");
    }

    #[test]
    fn inline_styles_win() {
        let html = r#"<body><p id="x" style="text-align: center">A</p></body>"#;
        assert_eq!(
            style_of("p { text-align: justify }", html, "x").align,
            Align::Center
        );
    }

    #[test]
    fn inheritance_and_relative_sizes() {
        let css = "blockquote { font-style: italic; font-size: 0.9em } .small { font-size: 80% }";
        let html = r#"<body><blockquote><p class="small" id="x">A</p></blockquote></body>"#;
        let s = style_of(css, html, "x");
        assert!(s.italic);
        assert!((s.size - 0.72).abs() < 0.001);
        assert_eq!(s.margin_left.em, 0.0, "margins don't inherit");
    }

    #[test]
    fn combinators_and_attributes() {
        let css = "div > p { text-align: center } h1 + p { text-indent: 0 } p ~ p { text-indent: 1em } \
                   aside[epub|type~=\"footnote\"] { display: none } a[href^='#n'] { vertical-align: super }";
        let html = r##"<body xmlns:epub="http://www.idpf.org/2007/ops"><div><p id="a">A</p></div>
            <h1>T</h1><p id="b">B</p><p id="c">C</p>
            <aside epub:type="footnote" id="n">N</aside><a href="#n1" id="r">1</a></body>"##;
        assert_eq!(style_of(css, html, "a").align, Align::Center);
        assert_eq!(style_of(css, html, "b").align, Align::Start);
        assert!(style_of(css, html, "b").text_indent.is_zero());
        assert_eq!(style_of(css, html, "c").text_indent.em, 1.0);
        assert_eq!(style_of(css, html, "n").display, Display::None);
        assert_eq!(style_of(css, html, "r").vertical, Vertical::Super);
    }

    #[test]
    fn first_child_and_unsupported_selectors() {
        let sheet = Stylesheet::new(&["p:first-child { text-indent: 0 } p::first-letter { font-size: 3em } a:hover { font-weight: bold }"]);
        assert_eq!(sheet.skipped, 2);
        let html = r#"<body><p id="a">A</p><p id="b">B</p></body>"#;
        let css = "p { text-indent: 1em } p:first-child { text-indent: 0 }";
        assert!(style_of(css, html, "a").text_indent.is_zero());
        assert_eq!(style_of(css, html, "b").text_indent.em, 1.0);
    }

    #[test]
    fn kindle_media_queries_are_skipped_and_ours_kept() {
        let css = "@charset \"utf-8\"; @font-face { font-family: X; src: url(x.otf) } \
                   @media amzn-kf8 { p { font-style: italic } } \
                   @media not amzn-kf8 { p { font-weight: bold } } \
                   @media screen { p { text-align: center } } /* a comment { } */";
        let s = style_of(css, r#"<body><p id="x">A</p></body>"#, "x");
        assert!(!s.italic);
        assert!(s.bold);
        assert_eq!(s.align, Align::Center);
    }

    #[test]
    fn defaults_give_html_its_meaning() {
        let html = r#"<body><h2 id="h">T</h2><pre id="pre">x</pre><em id="em">e</em><span id="s">s</span><style id="st">p{}</style></body>"#;
        assert!(style_of("", html, "h").bold);
        assert_eq!(style_of("", html, "h").display, Display::Block);
        let pre = style_of("", html, "pre");
        assert!(pre.monospace && pre.pre);
        assert!(style_of("", html, "em").italic);
        assert_eq!(style_of("", html, "s").display, Display::Inline);
        assert_eq!(style_of("", html, "st").display, Display::None);
    }

    #[test]
    fn shorthands() {
        let css = "p { margin: 1em 0 0.5em 2em; font: italic bold 12px/1.4 Courier, monospace; padding: 0 0 0 1em }";
        let s = style_of(css, r#"<body><p id="x">A</p></body>"#, "x");
        assert_eq!(
            (s.margin_top.em, s.margin_bottom.em, s.margin_left.em),
            (1.0, 0.5, 2.0)
        );
        assert!(s.italic && s.bold && s.monospace);
        assert_eq!(s.left().em, 3.0);
    }
}
