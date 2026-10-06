//! An EPUB's documents in reading order: the OPF's spine.

use std::io::Read;
use std::path::Path;

/// One spine document: its path in the archive, and its text.
pub struct Doc {
    pub href: String,
    pub text: String,
}

pub fn spine(path: &Path) -> Result<Vec<Doc>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: not an EPUB: {e}", path.display()))?;
    let mut read = |name: &str| -> Result<String, String> {
        let mut entry = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };

    let container = read("META-INF/container.xml")?;
    let doc = roxmltree::Document::parse(&container).map_err(|e| format!("container.xml: {e}"))?;
    let opf_path = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .ok_or("container.xml names no OPF")?
        .to_string();
    let base = opf_path
        .rsplit_once('/')
        .map_or(String::new(), |(b, _)| format!("{b}/"));

    let opf = read(&opf_path)?;
    let doc = roxmltree::Document::parse(&opf).map_err(|e| format!("{opf_path}: {e}"))?;
    let manifest: Vec<(String, String)> = doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|n| {
            Some((
                n.attribute("id")?.to_string(),
                n.attribute("href")?.to_string(),
            ))
        })
        .collect();
    let mut out = Vec::new();
    for idref in doc
        .descendants()
        .filter(|n| n.has_tag_name("itemref"))
        .filter_map(|n| n.attribute("idref"))
    {
        let Some((_, href)) = manifest.iter().find(|(id, _)| id == idref) else {
            return Err(format!(
                "the spine names `{idref}`, which the manifest lacks"
            ));
        };
        let href = format!("{base}{}", decode_href(href));
        let text = read(&href)?;
        out.push(Doc { href, text });
    }
    Ok(out)
}

/// `%20` and friends in manifest paths.
fn decode_href(href: &str) -> String {
    let bytes = href.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&href[i + 1..i + 3], 16) {
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

/// HTML's named entities that XML doesn't know, as characters, so the
/// document parses.
pub fn xml_entities(text: &str) -> String {
    const NAMED: &[(&str, &str)] = &[
        ("&nbsp;", "\u{a0}"),
        ("&mdash;", "—"),
        ("&ndash;", "–"),
        ("&hellip;", "…"),
        ("&lsquo;", "‘"),
        ("&rsquo;", "’"),
        ("&ldquo;", "“"),
        ("&rdquo;", "”"),
        ("&copy;", "©"),
        ("&shy;", "\u{ad}"),
        ("&thinsp;", "\u{2009}"),
        ("&ensp;", "\u{2002}"),
        ("&emsp;", "\u{2003}"),
        ("&eacute;", "é"),
        ("&aelig;", "æ"),
        ("&thorn;", "þ"),
        ("&eth;", "ð"),
    ];
    if !text.contains('&') {
        return text.to_string();
    }
    let mut s = text.to_string();
    for (name, c) in NAMED {
        if s.contains(name) {
            s = s.replace(name, c);
        }
    }
    s
}
