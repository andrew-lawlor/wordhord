//! Packs from DictFile, the format reader.dict publishes its Wiktionary
//! dictionaries in: `@ headword`, `: pronunciation`, `& inflected form`,
//! then the entry as one line of HTML.
//!
//! The HTML gives each part of speech as `<p><b>Noun</b></p>` and its senses
//! as a list; the etymology follows as plain paragraphs. Forms come without
//! their grammar: "went" is known to be a form of "go", not which form.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::{Builder, Sense};

/// Parts of speech, as the HTML names them. Other headings (Synonyms,
/// Antonyms) are left out.
const POS: [&str; 26] = [
    "noun",
    "proper noun",
    "verb",
    "adjective",
    "adverb",
    "pronoun",
    "preposition",
    "conjunction",
    "interjection",
    "determiner",
    "article",
    "numeral",
    "particle",
    "phrase",
    "prepositional phrase",
    "proverb",
    "idiom",
    "prefix",
    "suffix",
    "infix",
    "contraction",
    "abbreviation",
    "initialism",
    "acronym",
    "symbol",
    "letter",
];

/// Builds a pack from a DictFile, plain or bzip2-compressed. Returns how
/// many entries and forms went in.
pub fn build(path: &Path, language: &str, out: &Path) -> Result<(usize, usize), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let reader: Box<dyn Read> = if path.extension().is_some_and(|e| e == "bz2") {
        Box::new(bzip2::read::BzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut builder = Builder::new(out)?;
    let (mut word, mut pron, mut forms) =
        (String::new(), Vec::<String>::new(), Vec::<String>::new());
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if let Some(w) = line.strip_prefix("@ ") {
            word = w.trim().to_string();
            pron.clear();
            forms.clear();
        } else if let Some(p) = line.strip_prefix(": ") {
            pron.push(p.trim().to_string());
        } else if let Some(f) = line.strip_prefix("& ") {
            forms.push(f.trim().to_string());
        } else if let Some(html) = line.strip_prefix("<html>") {
            if word.is_empty() {
                continue;
            }
            let (parts, etymology) = parse(html);
            let head = match pron.first() {
                Some(p) => format!("{word}  {p}"),
                None => word.clone(),
            };
            for (pos, senses) in parts {
                if senses.is_empty() {
                    continue;
                }
                let form_refs: Vec<(String, String)> =
                    forms.iter().map(|f| (f.clone(), String::new())).collect();
                builder.add(
                    &word,
                    &pos,
                    &head,
                    &senses,
                    etymology.as_deref(),
                    &form_refs,
                )?;
            }
        }
    }
    builder.finish(language, path)
}

/// The entry's parts of speech with their senses, and its etymology.
fn parse(html: &str) -> (Vec<(String, Vec<Sense>)>, Option<String>) {
    let mut parts: Vec<(String, Vec<Sense>)> = Vec::new();
    let mut etymology = Vec::new();
    let mut rest = html;
    while let Some(p) = rest.find("<p>") {
        rest = &rest[p + 3..];
        let end = rest.find("</p>").unwrap_or(rest.len());
        let para = &rest[..end];
        rest = &rest[end..];
        if let Some(heading) = para
            .strip_prefix("<b>")
            .and_then(|h| h.strip_suffix("</b>"))
        {
            let heading = text(heading).to_lowercase();
            // The list that follows the heading.
            let list = rest
                .find("<ol>")
                .filter(|&k| rest[..k].trim_start_matches("</p>").trim().is_empty());
            let senses = match list {
                Some(k) => {
                    let (items, after) = top_items(&rest[k..]);
                    rest = after;
                    items
                        .into_iter()
                        .map(|i| sense(&i))
                        .filter(|s| !s.gloss.is_empty())
                        .collect()
                }
                None => Vec::new(),
            };
            if POS.contains(&heading.as_str()) {
                parts.push((heading, senses));
            }
        } else {
            let t = text(para);
            if !t.is_empty() {
                etymology.push(t);
            }
        }
    }
    let etymology = (!etymology.is_empty()).then(|| etymology.join(" "));
    (parts, etymology)
}

/// The `<li>` items of the list at the start of `html`, without the lists
/// nested in them; and what follows the list.
fn top_items(html: &str) -> (Vec<String>, &str) {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut k = 0;
    let bytes = html.as_bytes();
    while k < bytes.len() {
        let here = &html[k..];
        if here.starts_with("<ol") {
            depth += 1;
            k += here.find('>').map_or(here.len(), |e| e + 1);
            continue;
        }
        if here.starts_with("</ol>") {
            depth -= 1;
            k += 5;
            if depth == 0 {
                if !current.trim().is_empty() {
                    items.push(std::mem::take(&mut current));
                }
                return (items, &html[k..]);
            }
            continue;
        }
        if depth == 1 && here.starts_with("<li>") {
            if !current.trim().is_empty() {
                items.push(std::mem::take(&mut current));
            }
            k += 4;
            continue;
        }
        let ch = here.chars().next().unwrap_or(' ');
        if depth == 1 {
            current.push(ch);
        }
        k += ch.len_utf8();
    }
    (items, "")
}

/// A sense from a list item: a leading "(intransitive)" or "(billiards,
/// snooker)" becomes its tags.
fn sense(item: &str) -> Sense {
    let mut gloss = text(item);
    let mut tags = Vec::new();
    if gloss.starts_with('(') {
        if let Some(close) = gloss.find(')') {
            tags = gloss[1..close]
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            gloss = gloss[close + 1..].trim().to_string();
        }
    }
    Sense {
        gloss,
        tags,
        examples: Vec::new(),
        form_of: None,
    }
}

/// HTML as plain text: no tags, entities decoded, spaces collapsed.
fn text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = decode(&out);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('&') {
        out.push_str(&rest[..a]);
        rest = &rest[a..];
        let Some(semi) = rest.find(';').filter(|&k| k < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_of_speech_senses_tags_and_etymology() {
        let html = r#"<p><b>Noun</b></p><ol><li>A striped bovine mammal.</li><li>Either of a pair of small drums.</li><ol style="list-style-type:lower-alpha"><li>Synonym: bongo drum</li></ol></ol><p><b>Verb</b></p><ol><li>(<i>intransitive</i>) To play the bongo drums.</li></ol><p><b>Synonym</b></p><ol><li>timber wolf</li></ol><p>From Spanish <i>bongo</i>&#32;&amp; more.</p><br/>"#;
        let (parts, ety) = parse(html);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "noun");
        let glosses: Vec<&str> = parts[0].1.iter().map(|s| s.gloss.as_str()).collect();
        assert_eq!(
            glosses,
            [
                "A striped bovine mammal.",
                "Either of a pair of small drums."
            ]
        );
        assert_eq!(parts[1].1[0].tags, ["intransitive"]);
        assert_eq!(parts[1].1[0].gloss, "To play the bongo drums.");
        assert_eq!(ety.as_deref(), Some("From Spanish bongo & more."));
    }
}
