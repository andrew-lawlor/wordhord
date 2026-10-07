//! XHTML as EPUBs ship it. It should be XML, but it often uses HTML's named
//! entities (`&nbsp;`, `&mdash;`) without declaring them, which no XML
//! parser accepts. They're replaced by their characters first.

use std::borrow::Cow;

pub fn parse(text: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: u32::MAX,
        },
    )
}

/// The text with HTML's named entities replaced by their characters, and
/// bare ampersands escaped. XML's own five, and numeric references, are left
/// for the parser.
pub fn fix_entities(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .map(|k| k + 1);
        match end {
            Some(end) if rest[end..].starts_with(';') && end > 1 => {
                let name = &rest[1..end];
                match entity(name) {
                    Some(c) => {
                        out.push(c);
                        changed = true;
                    }
                    None if matches!(name, "amp" | "lt" | "gt" | "quot" | "apos") => {
                        out.push_str(&rest[..=end]);
                    }
                    // Unknown: keep it as text rather than fail the chapter.
                    None => {
                        out.push_str("&amp;");
                        out.push_str(&rest[1..=end]);
                        changed = true;
                    }
                }
                rest = &rest[end + 1..];
            }
            // A numeric reference is the parser's; a bare `&` is escaped.
            _ if rest[1..].starts_with('#') => {
                out.push('&');
                rest = &rest[1..];
            }
            _ => {
                out.push_str("&amp;");
                changed = true;
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    }
}

/// HTML's named entities that turn up in books.
fn entity(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => '\u{a0}',
        "shy" => '\u{ad}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "hairsp" => '\u{200a}',
        "zwnj" => '\u{200c}',
        "zwj" => '\u{200d}',
        "ndash" => '–',
        "mdash" => '—',
        "horbar" => '―',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "ldquo" => '“',
        "rdquo" => '”',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "lsaquo" => '‹',
        "rsaquo" => '›',
        "hellip" => '…',
        "middot" => '·',
        "bull" => '•',
        "prime" => '′',
        "Prime" => '″',
        "dagger" => '†',
        "Dagger" => '‡',
        "sect" => '§',
        "para" => '¶',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "minus" => '−',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup1" => '¹',
        "sup2" => '²',
        "sup3" => '³',
        "pound" => '£',
        "euro" => '€',
        "cent" => '¢',
        "yen" => '¥',
        "iexcl" => '¡',
        "iquest" => '¿',
        "ordf" => 'ª',
        "ordm" => 'º',
        "micro" => 'µ',
        "acute" => '´',
        "cedil" => '¸',
        "uml" => '¨',
        "macr" => '¯',
        "not" => '¬',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "AElig" => 'Æ',
        "aelig" => 'æ',
        "OElig" => 'Œ',
        "oelig" => 'œ',
        "ETH" => 'Ð',
        "eth" => 'ð',
        "THORN" => 'Þ',
        "thorn" => 'þ',
        "szlig" => 'ß',
        "Oslash" => 'Ø',
        "oslash" => 'ø',
        "Aring" => 'Å',
        "aring" => 'å',
        "Ccedil" => 'Ç',
        "ccedil" => 'ç',
        "Ntilde" => 'Ñ',
        "ntilde" => 'ñ',
        "scaron" => 'š',
        "Scaron" => 'Š',
        "zcaron" => 'ž',
        "Zcaron" => 'Ž',
        "alpha" => 'α',
        "beta" => 'β',
        "gamma" => 'γ',
        "delta" => 'δ',
        "epsilon" => 'ε',
        "lambda" => 'λ',
        "mu" => 'μ',
        "pi" => 'π',
        "sigma" => 'σ',
        "omega" => 'ω',
        "Omega" => 'Ω',
        _ => return accented(name),
    })
}

/// `&eacute;`, `&Ouml;` and the rest: a letter and an accent.
fn accented(name: &str) -> Option<char> {
    let (letter, accent) = name.split_at(1);
    let base = letter.chars().next()?;
    let mark = match accent {
        "grave" => '\u{300}',
        "acute" => '\u{301}',
        "circ" => '\u{302}',
        "tilde" => '\u{303}',
        "uml" => '\u{308}',
        "ring" => '\u{30a}',
        "cedil" => '\u{327}',
        _ => return None,
    };
    if !base.is_ascii_alphabetic() {
        return None;
    }
    compose(base, mark)
}

/// Latin-1 and Latin Extended-A letters with one accent.
fn compose(base: char, mark: char) -> Option<char> {
    const TABLE: &[(char, char, char)] = &[
        ('a', '\u{300}', 'à'),
        ('a', '\u{301}', 'á'),
        ('a', '\u{302}', 'â'),
        ('a', '\u{303}', 'ã'),
        ('a', '\u{308}', 'ä'),
        ('e', '\u{300}', 'è'),
        ('e', '\u{301}', 'é'),
        ('e', '\u{302}', 'ê'),
        ('e', '\u{308}', 'ë'),
        ('i', '\u{300}', 'ì'),
        ('i', '\u{301}', 'í'),
        ('i', '\u{302}', 'î'),
        ('i', '\u{308}', 'ï'),
        ('o', '\u{300}', 'ò'),
        ('o', '\u{301}', 'ó'),
        ('o', '\u{302}', 'ô'),
        ('o', '\u{303}', 'õ'),
        ('o', '\u{308}', 'ö'),
        ('u', '\u{300}', 'ù'),
        ('u', '\u{301}', 'ú'),
        ('u', '\u{302}', 'û'),
        ('u', '\u{308}', 'ü'),
        ('y', '\u{301}', 'ý'),
        ('y', '\u{308}', 'ÿ'),
        ('n', '\u{303}', 'ñ'),
        ('c', '\u{327}', 'ç'),
        ('A', '\u{300}', 'À'),
        ('A', '\u{301}', 'Á'),
        ('A', '\u{302}', 'Â'),
        ('A', '\u{303}', 'Ã'),
        ('A', '\u{308}', 'Ä'),
        ('E', '\u{300}', 'È'),
        ('E', '\u{301}', 'É'),
        ('E', '\u{302}', 'Ê'),
        ('E', '\u{308}', 'Ë'),
        ('I', '\u{300}', 'Ì'),
        ('I', '\u{301}', 'Í'),
        ('I', '\u{302}', 'Î'),
        ('I', '\u{308}', 'Ï'),
        ('O', '\u{300}', 'Ò'),
        ('O', '\u{301}', 'Ó'),
        ('O', '\u{302}', 'Ô'),
        ('O', '\u{303}', 'Õ'),
        ('O', '\u{308}', 'Ö'),
        ('U', '\u{300}', 'Ù'),
        ('U', '\u{301}', 'Ú'),
        ('U', '\u{302}', 'Û'),
        ('U', '\u{308}', 'Ü'),
        ('Y', '\u{301}', 'Ý'),
        ('N', '\u{303}', 'Ñ'),
        ('C', '\u{327}', 'Ç'),
        ('a', '\u{30a}', 'å'),
        ('A', '\u{30a}', 'Å'),
    ];
    TABLE
        .iter()
        .find(|(b, m, _)| *b == base && *m == mark)
        .map(|(_, _, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_entities_become_characters() {
        let fixed = fix_entities(
            "A&nbsp;B&mdash;C &eacute;t&eacute; &amp; &lt;x&gt; &#8217; &bogus; R & D",
        );
        assert_eq!(
            fixed,
            "A\u{a0}B—C été &amp; &lt;x&gt; &#8217; &amp;bogus; R &amp; D"
        );
        assert!(parse(&format!("<p>{fixed}</p>")).is_ok());
    }

    #[test]
    fn text_without_entities_is_untouched() {
        assert!(matches!(
            fix_entities("plain &amp; simple"),
            Cow::Borrowed(_)
        ));
    }
}
