//! Reads each EPUB given and reports what the engine makes of it: the
//! package, the contents, documents that don't parse, the CSS, and the
//! computed style of the most-used classes.
//!
//! cargo run --release -p wordhord-epub --example survey -- <book.epub>...

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use wordhord_epub::css::{Display, Style, Stylesheet};
use wordhord_epub::{xml, Epub};

fn main() {
    for path in std::env::args().skip(1) {
        let started = Instant::now();
        let epub = match Epub::open(Path::new(&path)) {
            Ok(e) => e,
            Err(e) => {
                println!("== {path}\n  FAILED: {e}");
                continue;
            }
        };
        let m = &epub.meta;
        println!(
            "== {} — {} [{}; EPUB {}]",
            m.title,
            m.creators.join(", "),
            m.language.as_deref().unwrap_or("?"),
            m.version
        );
        println!(
            "  spine {} ({} not linear); contents {} entries, {} deep",
            epub.spine.len(),
            epub.spine.iter().filter(|s| !s.linear).count(),
            epub.toc.len(),
            epub.toc.iter().map(|t| t.depth + 1).max().unwrap_or(0)
        );
        let mut broken = Vec::new();
        let (mut elements, mut rules, mut skipped) = (0, 0, 0);
        // Per class: how often, and one computed style.
        let mut classes: HashMap<String, (usize, Style, String)> = HashMap::new();
        let mut styled_italic = 0;
        let mut hidden = 0;
        for item in &epub.spine {
            let Ok(text) = epub.text(&item.path) else {
                broken.push(format!("{}: unreadable", item.path));
                continue;
            };
            let doc = match xml::parse(&text) {
                Ok(d) => d,
                Err(e) => {
                    broken.push(format!("{}: {e}", item.path));
                    continue;
                }
            };
            let sheet = Stylesheet::new(&epub.stylesheets(&item.path, &doc));
            rules = rules.max(sheet.rules());
            skipped = skipped.max(sheet.skipped);
            walk(
                doc.root_element(),
                &sheet,
                &Style::default(),
                &mut |n, s| {
                    elements += 1;
                    let tag = n.tag_name().name();
                    if s.italic && !matches!(tag, "i" | "em" | "cite" | "var" | "dfn" | "address") {
                        styled_italic += 1;
                    }
                    if s.display == Display::None {
                        hidden += 1;
                    }
                    for c in n.attribute("class").unwrap_or_default().split_whitespace() {
                        let e =
                            classes
                                .entry(c.to_string())
                                .or_insert((0, s.clone(), tag.to_string()));
                        e.0 += 1;
                    }
                },
            );
        }
        println!(
            "  {elements} elements in {:?}; CSS {rules} rules, {skipped} selectors skipped; \
             {styled_italic} italic by CSS alone; {hidden} hidden",
            started.elapsed()
        );
        for b in &broken {
            println!("  BROKEN {b}");
        }
        let mut top: Vec<_> = classes.into_iter().collect();
        top.sort_by_key(|(_, (n, _, _))| std::cmp::Reverse(*n));
        for (class, (n, s, tag)) in top.into_iter().take(8) {
            println!("    {tag}.{class} ×{n}: {}", describe(&s));
        }
    }
}

fn walk<'a, 'b>(
    node: roxmltree::Node<'a, 'b>,
    sheet: &Stylesheet,
    parent: &Style,
    f: &mut dyn FnMut(roxmltree::Node<'a, 'b>, &Style),
) {
    let style = sheet.style(node, parent);
    f(node, &style);
    for child in node.children().filter(|c| c.is_element()) {
        walk(child, sheet, &style, f);
    }
}

fn describe(s: &Style) -> String {
    let mut out = vec![format!("{:?}", s.display).to_lowercase()];
    for (on, word) in [
        (s.italic, "italic"),
        (s.bold, "bold"),
        (s.small_caps, "small caps"),
        (s.monospace, "mono"),
        (s.uppercase, "upper"),
        (s.pre, "pre"),
        (s.break_before, "page break"),
    ] {
        if on {
            out.push(word.to_string());
        }
    }
    if !s.text_indent.is_zero() {
        out.push(format!(
            "indent {:.2}em{}",
            s.text_indent.em,
            pct(s.text_indent.percent)
        ));
    }
    if !s.left().is_zero() {
        out.push(format!(
            "left {:.2}em{}",
            s.left().em,
            pct(s.left().percent)
        ));
    }
    if s.align != Default::default() {
        out.push(format!("{:?}", s.align).to_lowercase());
    }
    if (s.size - 1.0).abs() > 0.01 {
        out.push(format!("size {:.2}", s.size));
    }
    if s.vertical != Default::default() {
        out.push(format!("{:?}", s.vertical).to_lowercase());
    }
    out.join(", ")
}

fn pct(p: f32) -> String {
    if p.abs() > 0.001 {
        format!(" +{:.0}%", p * 100.0)
    } else {
        String::new()
    }
}
