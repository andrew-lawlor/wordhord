//! Each pattern the test books use, in a small EPUB built for the test.

use std::io::Write;

use super::*;
use wordhord_book::model::BlockKind;

const OPF: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test</dc:title><dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="c" href="ch.xhtml" media-type="application/xhtml+xml"/>
    <item id="n" href="notes.xhtml" media-type="application/xhtml+xml"/>
    <item id="toc" href="toc.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="css" href="style.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="c"/><itemref idref="n"/></spine>
</package>"#;

const CSS: &str = "
    div.p-indent { text-indent: 1.2em; margin: 0 }
    span.i { font-style: italic }
    p.pty { margin: 0 0 0 2.6em; text-indent: -1.3em }
    p.nonindent1, p.nonindent2 { text-indent: 0; margin: 0 }
    p.nonindent2 { margin-left: 3em }
    img.img { width: 10% }
";

/// One chapter, as blocks, from an EPUB holding `body`, a notes file and
/// a contents file.
fn convert(body: &str, notes: &str) -> Vec<Block> {
    let page = |body: &str| {
        format!(
            r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><link rel="stylesheet" href="style.css"/></head><body>{body}</body></html>"#
        )
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "wordhord-epub-{}-{}.epub",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let files = [
            ("mimetype", "application/epub+zip".to_string()),
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#.to_string(),
            ),
            ("content.opf", OPF.to_string()),
            ("style.css", CSS.to_string()),
            ("ch.xhtml", page(body)),
            ("notes.xhtml", page(notes)),
            ("toc.xhtml", page(r#"<nav epub:type="toc"><ol><li><a href="ch.xhtml">One</a></li></ol></nav>"#)),
        ];
        for (name, text) in files {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    let epub = Epub::open(&path).unwrap();
    let blocks = chapter(&epub, "ch.xhtml").unwrap();
    std::fs::remove_file(&path).unwrap();
    blocks
}

fn text(inlines: &[Inline]) -> String {
    plain_text(inlines)
}

fn note(blocks: &[Block]) -> Option<(String, Vec<Block>)> {
    blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Note { label, blocks } => Some((label.clone(), blocks.clone())),
        _ => None,
    })
}

#[test]
fn classes_make_paragraphs_and_italics() {
    let blocks = convert(
        r#"<div class="p-indent">He saw <span class="i">Ragnar</span> come.</div>
           <div class="p-indent">Then he ran.</div>"#,
        "",
    );
    assert_eq!(blocks.len(), 2);
    let BlockKind::Paragraph(inlines) = &blocks[0].kind else {
        panic!("{:?}", blocks[0])
    };
    assert_eq!(text(inlines), "He saw Ragnar come.");
    assert!(inlines
        .iter()
        .any(|i| matches!(i, Inline::Emphasis(e) if text(e) == "Ragnar")));
}

#[test]
fn a_hanging_indent_is_verse_with_line_numbers() {
    let blocks = convert(
        r#"<div><p class="pty">Tell me about a complicated man.</p>
           <p class="pty">Muse, tell me how he wandered and was lost</p>
           <p class="pty">tell the old story for our modern times.<span>10</span></p></div>"#,
        "",
    );
    let BlockKind::Verse(v) = &blocks[0].kind else {
        panic!("{blocks:?}")
    };
    assert_eq!(v.lines.len(), 3);
    assert_eq!(v.lines[2].reference.as_deref(), Some("10"));
    assert_eq!(
        text(&v.lines[2].inlines),
        "tell the old story for our modern times."
    );
}

#[test]
fn numbered_stanzas_and_an_image_drop_cap() {
    let blocks = convert(
        r#"<p class="nonindent1"><span>1</span> <img class="img" src="h.jpg" alt="Capital letter H decorated with a tree. Heed"/><span><span aria-hidden="true">EED</span> MY WORDS</span>,</p>
           <p class="nonindent2">all holy kindreds,</p>
           <p class="nonindent2">you greater and lesser</p>
           <p class="nonindent1"><span>2</span> I remember the jotuns</p>
           <p class="nonindent2">born so long ago;</p>"#,
        "",
    );
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    let BlockKind::Stanza(v) = &blocks[0].kind else {
        panic!("{blocks:?}")
    };
    assert_eq!(blocks[0].reference.as_deref(), Some("1"));
    assert!(
        text(&v.lines[0].inlines).starts_with("HEED MY WORDS"),
        "{:?}",
        v.lines[0]
    );
    assert_eq!(blocks[1].reference.as_deref(), Some("2"));
}

#[test]
fn bracketed_line_numbers_go_to_the_margin() {
    let blocks = convert(
        r#"<p class="pty">with the weapons of a warrior,</p>
           <p class="pty"><span>[40]</span> swords and body-armour;</p>
           <p class="pty">treasures and trappings</p>"#,
        "",
    );
    let BlockKind::Verse(v) = &blocks[0].kind else {
        panic!("{blocks:?}")
    };
    assert_eq!(v.lines[1].reference.as_deref(), Some("40"));
    assert_eq!(text(&v.lines[1].inlines), "swords and body-armour;");
}

#[test]
fn notes_in_another_file_lose_their_back_link() {
    let blocks = convert(
        r#"<section epub:type="z3998:poem"><h2>The Prologue</h2>
           <p><span>When that Aprilis, with his showers swoot,<a href="notes.xhtml#n1" id="r1" epub:type="noteref">1</a></span></p></section>"#,
        r#"<ol><li id="n1"><p>Sweet. <a href="ch.xhtml#r1" epub:type="backlink">↩</a></p></li></ol>"#,
    );
    assert!(
        matches!(&blocks[0].kind, BlockKind::Heading { text: t, .. } if plain_text(t) == "The Prologue"),
        "a heading inside a poem stays a heading: {blocks:?}"
    );
    let (label, body) = note(&blocks).expect("a note");
    assert_eq!(label, "1");
    let BlockKind::Paragraph(inlines) = &body[0].kind else {
        panic!("{body:?}")
    };
    assert_eq!(text(inlines), "Sweet.");
}

#[test]
fn a_note_anchored_mid_paragraph_takes_the_paragraph_without_its_number() {
    let blocks = convert(
        r##"<p>The gods met.<sup><a href="notes.xhtml#fn3">3</a></sup></p>"##,
        r##"<p class="footnote"><a id="fn3" href="ch.xhtml#r3">3</a> Þrúðr: a Valkyrie.</p>"##,
    );
    let (label, body) = note(&blocks).expect("a note");
    assert_eq!(label, "3");
    let BlockKind::Paragraph(inlines) = &body[0].kind else {
        panic!("{body:?}")
    };
    assert_eq!(text(inlines), "Þrúðr: a Valkyrie.");
}

#[test]
fn a_chapter_number_linking_to_the_contents_is_not_a_note() {
    let blocks = convert(
        r##"<h1><a href="toc.xhtml#Ref_1a" id="Ref_1"><span>1</span></a> The Home of Their Shapes</h1><p>Text.</p>"##,
        "",
    );
    assert!(note(&blocks).is_none(), "{blocks:?}");
}

#[test]
fn short_dialogue_stays_prose() {
    let blocks = convert(
        r#"<div class="p-indent">"Yes," he said.</div><div class="p-indent">"No."</div>
           <div class="p-indent">"Why?"</div><div class="p-indent">"Because."</div>"#,
        "",
    );
    assert!(
        blocks
            .iter()
            .all(|b| matches!(b.kind, BlockKind::Paragraph(_))),
        "{blocks:?}"
    );
}

#[test]
fn code_and_lists() {
    let blocks = convert(
        "<pre><code>func main() {\n    fmt.Println(\"hi\")\n}</code></pre>\
         <ul><li>One <code>nil</code> case.</li><li>Two.</li></ul>",
        "",
    );
    assert!(
        matches!(&blocks[0].kind, BlockKind::Code(c) if c.contains("\n    fmt.Println")),
        "{blocks:?}"
    );
    let BlockKind::List { ordered, items, .. } = &blocks[1].kind else {
        panic!("{blocks:?}")
    };
    assert!(!ordered);
    assert_eq!(items.len(), 2);
}
