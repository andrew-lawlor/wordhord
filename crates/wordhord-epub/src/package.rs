//! An EPUB's package: what the book is, its files, the order of its
//! chapters, and its table of contents (EPUB 3's nav document, or EPUB 2's
//! NCX).

use std::cell::RefCell;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::xml;

pub struct Epub {
    zip: RefCell<zip::ZipArchive<File>>,
    pub meta: Meta,
    pub manifest: Vec<Item>,
    /// Documents in reading order.
    pub spine: Vec<SpineItem>,
    pub toc: Vec<TocEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub title: String,
    pub creators: Vec<String>,
    /// As the book gives it: `en`, `en-GB`, `non`.
    pub language: Option<String>,
    pub identifier: Option<String>,
    /// 2.0 or 3.0.
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    /// Its path in the archive.
    pub path: String,
    pub media_type: String,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpineItem {
    pub path: String,
    /// `linear="no"`: outside the reading order, reached by links (notes).
    pub linear: bool,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub label: String,
    /// The archive path, with any `#fragment`.
    pub target: String,
    pub depth: usize,
}

impl Epub {
    pub fn open(path: &Path) -> Result<Epub, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let zip = zip::ZipArchive::new(file)
            .map_err(|e| format!("{}: not an EPUB: {e}", path.display()))?;
        let mut epub = Epub {
            zip: RefCell::new(zip),
            meta: Meta::default(),
            manifest: Vec::new(),
            spine: Vec::new(),
            toc: Vec::new(),
        };
        let container = epub.text("META-INF/container.xml")?;
        let doc = xml::parse(&container).map_err(|e| format!("container.xml: {e}"))?;
        let opf_path = doc
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .and_then(|n| n.attribute("full-path"))
            .ok_or("container.xml names no package")?
            .to_string();
        epub.read_package(&opf_path)?;
        epub.toc = epub.read_toc(&opf_path).unwrap_or_default();
        Ok(epub)
    }

    /// A file in the archive, as bytes.
    pub fn bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut zip = self.zip.borrow_mut();
        // Some books differ in case from their own manifest.
        let name = if zip.index_for_name(path).is_some() {
            path.to_string()
        } else {
            let lower = path.to_lowercase();
            (0..zip.len())
                .filter_map(|k| zip.name_for_index(k).map(str::to_string))
                .find(|n| n.to_lowercase() == lower)
                .ok_or_else(|| format!("{path}: not in the book"))?
        };
        let mut entry = zip.by_name(&name).map_err(|e| format!("{path}: {e}"))?;
        let mut out = Vec::new();
        entry
            .read_to_end(&mut out)
            .map_err(|e| format!("{path}: {e}"))?;
        Ok(out)
    }

    /// A file in the archive, as text, with HTML entities made XML-safe.
    pub fn text(&self, path: &str) -> Result<String, String> {
        let bytes = self.bytes(path)?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        Ok(xml::fix_entities(text).into_owned())
    }

    /// The manifest item at a path.
    pub fn item(&self, path: &str) -> Option<&Item> {
        self.manifest.iter().find(|i| i.path == path)
    }

    /// The stylesheets a document links to, as text, in order.
    pub fn stylesheets(&self, doc_path: &str, doc: &roxmltree::Document) -> Vec<String> {
        let mut out = Vec::new();
        for n in doc.descendants() {
            if n.has_tag_name("link")
                && n.attribute("rel").is_some_and(|r| {
                    r.split_whitespace()
                        .any(|r| r.eq_ignore_ascii_case("stylesheet"))
                })
            {
                if let Some(href) = n.attribute("href") {
                    if let Ok(css) = self.text(&resolve(doc_path, href)) {
                        out.push(css);
                    }
                }
            } else if n.has_tag_name("style") {
                out.push(n.text().unwrap_or_default().to_string());
            }
        }
        out
    }

    fn read_package(&mut self, opf_path: &str) -> Result<(), String> {
        let opf = self.text(opf_path)?;
        let doc = xml::parse(&opf).map_err(|e| format!("{opf_path}: {e}"))?;
        let root = doc.root_element();
        self.meta.version = root.attribute("version").unwrap_or("2.0").to_string();
        let text_of = |name: &str| {
            doc.descendants()
                .filter(|n| n.tag_name().name() == name && n.tag_name().namespace() == Some(DC))
                .map(|n| n.text().unwrap_or_default().trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
        };
        self.meta.title = text_of("title").into_iter().next().unwrap_or_default();
        self.meta.creators = text_of("creator");
        self.meta.language = text_of("language").into_iter().next();
        self.meta.identifier = text_of("identifier").into_iter().next();
        self.manifest = doc
            .descendants()
            .filter(|n| n.has_tag_name("item"))
            .filter_map(|n| {
                Some(Item {
                    id: n.attribute("id")?.to_string(),
                    path: resolve(opf_path, n.attribute("href")?),
                    media_type: n.attribute("media-type").unwrap_or_default().to_string(),
                    properties: n
                        .attribute("properties")
                        .unwrap_or_default()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect(),
                })
            })
            .collect();
        self.spine = doc
            .descendants()
            .filter(|n| n.has_tag_name("itemref"))
            .filter_map(|n| {
                let item = self
                    .manifest
                    .iter()
                    .find(|i| Some(i.id.as_str()) == n.attribute("idref"))?;
                Some(SpineItem {
                    path: item.path.clone(),
                    linear: n.attribute("linear") != Some("no"),
                })
            })
            .collect();
        if self.spine.is_empty() {
            return Err(format!("{opf_path}: no spine"));
        }
        Ok(())
    }

    fn read_toc(&self, opf_path: &str) -> Option<Vec<TocEntry>> {
        // EPUB 3: the nav document.
        if let Some(nav) = self
            .manifest
            .iter()
            .find(|i| i.properties.iter().any(|p| p == "nav"))
        {
            let text = self.text(&nav.path).ok()?;
            let doc = xml::parse(&text).ok()?;
            let toc = doc.descendants().find(|n| {
                n.has_tag_name("nav")
                    && n.attributes().any(|a| {
                        a.name() == "type" && a.value().split_whitespace().any(|t| t == "toc")
                    })
            });
            if let Some(toc) = toc {
                let mut out = Vec::new();
                nav_entries(toc, &nav.path, 0, &mut out);
                if !out.is_empty() {
                    return Some(out);
                }
            }
        }
        // EPUB 2: the NCX.
        let opf = self.text(opf_path).ok()?;
        let doc = xml::parse(&opf).ok()?;
        let ncx_id = doc
            .descendants()
            .find(|n| n.has_tag_name("spine"))
            .and_then(|n| n.attribute("toc"));
        let ncx = self.manifest.iter().find(|i| {
            Some(i.id.as_str()) == ncx_id || i.media_type == "application/x-dtbncx+xml"
        })?;
        let text = self.text(&ncx.path).ok()?;
        let doc = xml::parse(&text).ok()?;
        let map = doc.descendants().find(|n| n.has_tag_name("navMap"))?;
        let mut out = Vec::new();
        ncx_entries(map, &ncx.path, 0, &mut out);
        Some(out)
    }
}

const DC: &str = "http://purl.org/dc/elements/1.1/";

fn nav_entries(node: roxmltree::Node, base: &str, depth: usize, out: &mut Vec<TocEntry>) {
    for li in node.descendants().filter(|n| n.has_tag_name("li")) {
        // Only this list's own items: an item's depth is how many lists
        // hold it.
        let lists = li
            .ancestors()
            .take_while(|a| *a != node)
            .filter(|a| a.has_tag_name("ol") || a.has_tag_name("ul"))
            .count();
        let Some(a) = li
            .children()
            .find(|c| c.has_tag_name("a") || c.has_tag_name("span"))
        else {
            continue;
        };
        let label = text_content(a);
        let target = a
            .attribute("href")
            .map(|h| resolve(base, h))
            .unwrap_or_default();
        if !label.is_empty() {
            out.push(TocEntry {
                label,
                target,
                depth: depth + lists.saturating_sub(1),
            });
        }
    }
}

fn ncx_entries(node: roxmltree::Node, base: &str, depth: usize, out: &mut Vec<TocEntry>) {
    for point in node.children().filter(|n| n.has_tag_name("navPoint")) {
        let label = point
            .children()
            .find(|n| n.has_tag_name("navLabel"))
            .map(text_content)
            .unwrap_or_default();
        let target = point
            .children()
            .find(|n| n.has_tag_name("content"))
            .and_then(|n| n.attribute("src"))
            .map(|s| resolve(base, s))
            .unwrap_or_default();
        if !label.is_empty() {
            out.push(TocEntry {
                label,
                target,
                depth,
            });
        }
        ncx_entries(point, base, depth + 1, out);
    }
}

fn text_content(node: roxmltree::Node) -> String {
    let text: String = node
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A link's target as an archive path: relative to the file it's in,
/// with `..` resolved and `%20` decoded. A `#fragment` is kept.
pub fn resolve(from: &str, href: &str) -> String {
    let (path, fragment) = href
        .split_once('#')
        .map_or((href, None), |(p, f)| (p, Some(f)));
    let path = decode(path);
    let mut parts: Vec<&str> = if path.is_empty() {
        from.split('/').collect()
    } else if let Some(abs) = path.strip_prefix('/') {
        abs.split('/').collect()
    } else {
        let mut base: Vec<&str> = from.split('/').collect();
        base.pop();
        base.extend(path.split('/'));
        base
    };
    let mut out: Vec<&str> = Vec::new();
    for p in parts.drain(..) {
        match p {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    let joined = out.join("/");
    match fragment {
        Some(f) => format!("{joined}#{f}"),
        None => joined,
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_resolve_against_their_file() {
        assert_eq!(
            resolve("OEBPS/content.opf", "Text/ch01.xhtml"),
            "OEBPS/Text/ch01.xhtml"
        );
        assert_eq!(
            resolve("OEBPS/Text/ch01.xhtml", "../Styles/a.css"),
            "OEBPS/Styles/a.css"
        );
        assert_eq!(
            resolve("OEBPS/Text/ch01.xhtml", "notes.xhtml#n4"),
            "OEBPS/Text/notes.xhtml#n4"
        );
        assert_eq!(
            resolve("OEBPS/Text/ch01.xhtml", "#n4"),
            "OEBPS/Text/ch01.xhtml#n4"
        );
        assert_eq!(
            resolve("content.opf", "My%20Chapter.xhtml"),
            "My Chapter.xhtml"
        );
        assert_eq!(resolve("a/b.xhtml", "/c.xhtml"), "c.xhtml");
    }
}
