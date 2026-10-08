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
    /// Where the text proper begins, past the cover, title page and the
    /// rest, as the book marks it: EPUB 3's `bodymatter` landmark, or EPUB
    /// 2's guide reference of type `text`. An archive path, no fragment.
    pub body: Option<String>,
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
    /// The title to sort by: "Hammer and the Cross, The".
    pub title_sort: Option<String>,
    /// The first author, to sort by: "Harrison, Harry".
    pub author_sort: Option<String>,
    pub series: Option<String>,
    /// The book's number in its series: 1, 2, 2.5.
    pub series_index: Option<f32>,
    /// Subjects: Calibre's tags.
    pub tags: Vec<String>,
    /// The cover image's archive path, once the manifest is read.
    pub cover: Option<String>,
}

/// What a package document says about its book, from Dublin Core,
/// Calibre's `calibre:` metadata, and EPUB 3's refinements. A Calibre
/// library's `metadata.opf` is read the same way.
pub fn read_meta(doc: &roxmltree::Document) -> Meta {
    let mut meta = Meta {
        version: doc
            .root_element()
            .attribute("version")
            .unwrap_or("2.0")
            .to_string(),
        ..Meta::default()
    };
    let dc = |name: &str| {
        doc.descendants()
            .filter(|n| n.tag_name().name() == name && n.tag_name().namespace() == Some(DC))
            .collect::<Vec<_>>()
    };
    let text = |n: &roxmltree::Node| n.text().unwrap_or_default().trim().to_string();
    meta.title = dc("title").first().map(text).unwrap_or_default();
    let creators = dc("creator");
    meta.creators = creators
        .iter()
        .map(text)
        .filter(|t| !t.is_empty())
        .collect();
    meta.language = dc("language").first().map(text).filter(|t| !t.is_empty());
    meta.identifier = dc("identifier").first().map(text).filter(|t| !t.is_empty());
    meta.tags = dc("subject")
        .iter()
        .map(text)
        .filter(|t| !t.is_empty())
        .collect();
    // `<meta name="calibre:series" content="…"/>`, and EPUB 3's
    // `<meta property="…" refines="#id">…</meta>`.
    let named = |name: &str| {
        doc.descendants()
            .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some(name))
            .and_then(|n| n.attribute("content"))
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
    };
    let refining = |id: Option<&str>, property: &str| {
        let id = id?;
        doc.descendants()
            .find(|n| {
                n.has_tag_name("meta")
                    && n.attribute("property") == Some(property)
                    && n.attribute("refines") == Some(&format!("#{id}"))
            })
            .map(|n| text(&n))
    };
    meta.title_sort = named("calibre:title_sort");
    meta.author_sort = creators.first().and_then(|c| {
        c.attributes()
            .find(|a| a.name() == "file-as")
            .map(|a| a.value().to_string())
            .or_else(|| refining(c.attribute("id"), "file-as"))
    });
    meta.series = named("calibre:series");
    meta.series_index = named("calibre:series_index").and_then(|n| n.parse().ok());
    if meta.series.is_none() {
        if let Some(c) = doc.descendants().find(|n| {
            n.has_tag_name("meta") && n.attribute("property") == Some("belongs-to-collection")
        }) {
            let kind = refining(c.attribute("id"), "collection-type");
            if kind.as_deref().is_none_or(|k| k == "series") {
                meta.series = Some(text(&c)).filter(|t| !t.is_empty());
                meta.series_index =
                    refining(c.attribute("id"), "group-position").and_then(|n| n.parse().ok());
            }
        }
    }
    meta
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
            body: None,
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
        epub.body = epub.read_body(&opf_path);
        Ok(epub)
    }

    /// A file's size in the archive, unpacked: for weighing chapters.
    pub fn size(&self, path: &str) -> Option<u64> {
        self.zip.borrow_mut().by_name(path).ok().map(|e| e.size())
    }

    fn read_body(&self, opf_path: &str) -> Option<String> {
        let strip = |t: String| t.split('#').next().unwrap_or_default().to_string();
        // EPUB 3: the nav document's landmarks.
        if let Some(nav) = self
            .manifest
            .iter()
            .find(|i| i.properties.iter().any(|p| p == "nav"))
        {
            let text = self.text(&nav.path).ok()?;
            let doc = xml::parse(&text).ok()?;
            let body = doc
                .descendants()
                .filter(|n| n.has_tag_name("a"))
                .find(|n| {
                    n.attributes().any(|a| {
                        a.name() == "type"
                            && a.value().split_whitespace().any(|t| t == "bodymatter")
                    })
                })
                .and_then(|n| n.attribute("href"));
            if let Some(href) = body {
                return Some(strip(resolve(&nav.path, href)));
            }
        }
        // EPUB 2: the guide.
        let opf = self.text(opf_path).ok()?;
        let doc = xml::parse(&opf).ok()?;
        let href = doc
            .descendants()
            .filter(|n| n.has_tag_name("reference"))
            .find(|n| {
                n.attribute("type")
                    .is_some_and(|t| t.eq_ignore_ascii_case("text"))
            })
            .and_then(|n| n.attribute("href"))?;
        Some(strip(resolve(opf_path, href)))
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

    /// Whether the book is locked by DRM: Adobe's `rights.xml`, or anything
    /// but a font named in `encryption.xml`. (Fonts are often obfuscated, a
    /// different and harmless thing.)
    pub fn drm(&self) -> bool {
        if self.bytes("META-INF/rights.xml").is_ok() {
            return true;
        }
        let Ok(text) = self.text("META-INF/encryption.xml") else {
            return false;
        };
        let Ok(doc) = xml::parse(&text) else {
            return true;
        };
        doc.descendants()
            .filter(|n| n.has_tag_name("CipherReference"))
            .filter_map(|n| n.attribute("URI"))
            .any(|uri| {
                let uri = uri.to_lowercase();
                ![".otf", ".ttf", ".woff", ".woff2"]
                    .iter()
                    .any(|f| uri.ends_with(f))
            })
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
        self.meta = read_meta(&doc);
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
        // The cover: EPUB 3's `cover-image`, EPUB 2's `<meta name="cover">`,
        // or an image called cover.
        let named = doc
            .descendants()
            .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))
            .and_then(|n| n.attribute("content"));
        let image = |i: &&Item| i.media_type.starts_with("image/");
        self.meta.cover = self
            .manifest
            .iter()
            .find(|i| i.properties.iter().any(|p| p == "cover-image"))
            .or_else(|| {
                self.manifest
                    .iter()
                    .filter(image)
                    .find(|i| Some(i.id.as_str()) == named)
            })
            .or_else(|| {
                self.manifest.iter().filter(image).find(|i| {
                    i.id.to_lowercase().contains("cover") || i.path.to_lowercase().contains("cover")
                })
            })
            .map(|i| i.path.clone());
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
