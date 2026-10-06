use std::path::{Path, PathBuf};

use wordhord_book::align::align;
use wordhord_book::model::*;
use wordhord_book::{Book, Severity};

fn fixture(name: &str) -> Book {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    Book::load(&dir).expect("fixture loads")
}

/// A book in a fresh temporary folder, from (path, contents) pairs.
fn book(files: &[(&str, &str)]) -> Book {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "wordhord-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let b = Book::load(&dir).expect("book loads");
    std::fs::remove_dir_all(&dir).unwrap();
    b
}

const LINE_BOOK: &str =
    "id = \"t\"\ntitle = \"T\"\nlang = \"eng\"\n[refs]\nprefix = \"bw\"\nunit = \"line\"\n";

fn messages(b: &Book, severity: Severity) -> Vec<String> {
    b.diagnostics
        .iter()
        .filter(|d| d.severity == severity)
        .map(|d| d.message.clone())
        .collect()
}

fn has(b: &Book, severity: Severity, needle: &str) -> bool {
    messages(b, severity).iter().any(|m| m.contains(needle))
}

fn verse_lines(b: &Book) -> Vec<VerseLine> {
    b.blocks()
        .into_iter()
        .flat_map(|(_, b)| match &b.kind {
            BlockKind::Verse(v) | BlockKind::Stanza(v) => v.lines.clone(),
            _ => vec![],
        })
        .collect()
}

#[test]
fn the_fixtures_are_clean() {
    for name in ["beowulf-ang", "beowulf-gummere", "edda-non", "edda-bellows"] {
        let b = fixture(name);
        assert!(b.diagnostics.is_empty(), "{name}: {:#?}", b.diagnostics);
    }
}

#[test]
fn counted_verse_is_numbered_from_start() {
    let refs: Vec<_> = verse_lines(&fixture("beowulf-ang"))
        .into_iter()
        .map(|l| l.reference.unwrap())
        .collect();
    assert_eq!(refs.len(), 11);
    assert_eq!(refs[0], "bw.1");
    assert_eq!(refs[10], "bw.11");
}

#[test]
fn a_caesura_splits_the_line_and_names_are_tagged() {
    let first = &verse_lines(&fixture("beowulf-ang"))[0];
    assert_eq!(
        plain_text(&first.inlines),
        "Hwæt! We Gardena | in geardagum,"
    );
    assert!(first.inlines.contains(&Inline::Caesura));
    assert!(first.inlines.iter().any(|i| matches!(i,
        Inline::Span { kind: SpanKind::Name { kind: NameKind::Group, key: Some(k), .. }, .. } if k == "spear-danes")));
}

#[test]
fn stanzas_number_their_own_lines() {
    let refs: Vec<_> = verse_lines(&fixture("edda-non"))
        .into_iter()
        .map(|l| l.reference.unwrap())
        .collect();
    assert_eq!(
        refs,
        (1..=8).map(|k| format!("vsp.1.{k}")).collect::<Vec<_>>()
    );
}

#[test]
fn a_translation_aligns_line_to_line() {
    let a = align(&fixture("beowulf-gummere"), &fixture("beowulf-ang"));
    assert!(a.diagnostics.is_empty(), "{:#?}", a.diagnostics);
    assert_eq!(a.pairs.len(), 11);
    assert_eq!(a.pairs[3].reference, "bw.4");
    assert_eq!(
        a.pairs[3].translation,
        ["Oft Scyld the Scefing from squadroned foes,"]
    );
    assert_eq!(
        a.pairs[3].original,
        ["Oft Scyld Scefing | sceaþena þreatum,"]
    );
}

#[test]
fn a_translation_aligns_stanza_to_stanza() {
    let a = align(&fixture("edda-bellows"), &fixture("edda-non"));
    assert!(a.diagnostics.is_empty(), "{:#?}", a.diagnostics);
    assert_eq!(a.pairs.len(), 1);
    assert_eq!(a.pairs[0].translation.len(), 4);
    assert_eq!(a.pairs[0].original.len(), 8);
}

#[test]
fn an_override_covers_two_lines_and_counting_continues() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        ("text/01.dj", "{#ch01-0001 ref=bw start=11}\n::: verse\nEleven\n[Twelve and thirteen]{n=12-13}\nFourteen\n:::\n"),
    ]);
    assert!(b.diagnostics.is_empty(), "{:#?}", b.diagnostics);
    let refs: Vec<_> = verse_lines(&b)
        .into_iter()
        .map(|l| l.reference.unwrap())
        .collect();
    assert_eq!(refs, ["bw.11", "bw.12-13", "bw.14"]);
}

#[test]
fn leading_spaces_are_indent_steps() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        (
            "text/01.dj",
            "{#ch01-0001}\n::: verse\nTell me, O Muse,\n  who travelled far\n    and wide\n:::\n",
        ),
    ]);
    let indents: Vec<_> = verse_lines(&b).into_iter().map(|l| l.indent).collect();
    assert_eq!(indents, [0, 1, 2]);
}

#[test]
fn blank_lines_in_verse_start_new_groups() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        (
            "text/01.dj",
            "{#ch01-0001}\n::: verse\nOne\nTwo\n\nThree\n:::\n",
        ),
    ]);
    let groups: Vec<_> = verse_lines(&b).into_iter().map(|l| l.group).collect();
    assert_eq!(groups, [0, 0, 1]);
}

#[test]
fn an_unquoted_dot_is_caught() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        (
            "text/01.dj",
            "{#ch01-0001 ref=vsp.1}\n::: stanza\nHearing I ask\n:::\n",
        ),
    ]);
    assert!(
        has(&b, Severity::Error, "attribute line wasn't read"),
        "{:#?}",
        b.diagnostics
    );
    assert!(
        has(&b, Severity::Error, "didn't open a block"),
        "{:#?}",
        b.diagnostics
    );
}

#[test]
fn things_outside_the_vocabulary_are_errors() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        ("text/01.dj", "{#ch01-0001}\n::: sidebar\nText.\n:::\n\n{#ch01-0002}\n- a list\n\n{#ch01-0003}\nA [span]{.mystery} and [bare].\n"),
    ]);
    assert!(has(
        &b,
        Severity::Error,
        "`::: sidebar` isn't in the vocabulary"
    ));
    assert!(has(&b, Severity::Error, "a list isn't in the vocabulary"));
    assert!(has(
        &b,
        Severity::Error,
        "`.mystery` isn't in the vocabulary"
    ));
}

#[test]
fn ids_are_required_and_unique() {
    let b = book(&[
        ("book.toml", LINE_BOOK),
        (
            "text/01.dj",
            "No id.\n\n{#ch01-0002}\nOne.\n\n{#ch01-0002}\nTwo.\n\n{#Chapter1}\nThree.\n",
        ),
    ]);
    assert!(has(&b, Severity::Error, "has no #id"));
    assert!(has(&b, Severity::Error, "#ch01-0002 is already used"));
    assert!(has(&b, Severity::Warning, "#Chapter1 isn't in the form"));
}

#[test]
fn names_must_be_in_the_register_with_the_right_kind() {
    let names = "[odinn]\nname = \"Óðinn\"\nkind = \"person\"\n\n[asgard]\nname = \"Ásgarðr\"\nkind = \"place\"\n";
    let b = book(&[
        ("book.toml", LINE_BOOK),
        ("names.toml", names),
        ("text/01.dj", "{#ch01-0001}\n[Odin]{.place key=odinn} met [Thor]{.person key=thor} and [Loki]{.person}.\n"),
    ]);
    assert!(has(
        &b,
        Severity::Error,
        "`odinn` is a person in names.toml, but tagged `.place`"
    ));
    assert!(has(&b, Severity::Error, "`key=thor` isn't in names.toml"));
    assert!(has(&b, Severity::Warning, "without `key=`"));
    assert!(has(&b, Severity::Warning, "[asgard] isn't used"));
}

#[test]
fn source_pages_and_prose_sections() {
    let toml = "id = \"nh\"\ntitle = \"Natural History\"\nlang = \"eng\"\n[refs]\nprefix = \"nh\"\nlevels = [\"book\", \"section\"]\n";
    let b = book(&[
        ("book.toml", toml),
        ("text/01.dj", "{#ch12-0001 ref=\"nh.12.41\"}\nThe prow[]{.pb n=25}ess. []{.ref n=\"nh.12.42\"}Cinnamon.\n\n{#ch12-0002 ref=\"nh.12\"}\nToo short.\n"),
    ]);
    assert!(has(&b, Severity::Warning, "no [edition]"));
    assert!(has(&b, Severity::Error, "`nh.12` should have 2 numbers"));
    let (_, first) = b.blocks()[0];
    let BlockKind::Paragraph(inlines) = &first.kind else {
        panic!()
    };
    assert!(inlines.contains(&Inline::PageBreak {
        n: "25".into(),
        edition: None
    }));
    assert!(inlines.contains(&Inline::SectionStart {
        reference: "nh.12.42".into()
    }));
    assert_eq!(plain_text(inlines), "The prowess. Cinnamon.");
}

#[test]
fn alignment_reports_skips_and_overruns() {
    let original = book(&[
        (
            "book.toml",
            "id = \"o\"\ntitle = \"O\"\nlang = \"ang\"\n[refs]\nprefix = \"bw\"\nunit = \"line\"\n",
        ),
        (
            "text/01.dj",
            "{#ch01-0001 ref=bw start=1}\n::: verse\nOne\nTwo\nThree\n:::\n",
        ),
    ]);
    let translation = book(&[
        ("book.toml", "id = \"t\"\ntitle = \"T\"\nlang = \"eng\"\noriginal = \"o\"\n[refs]\nprefix = \"bw\"\nunit = \"line\"\n"),
        ("text/01.dj", "{#ch01-0001 ref=bw start=2}\n::: verse\nTwo\nThree\nFour\n:::\n"),
    ]);
    let a = align(&translation, &original);
    let msgs: Vec<_> = a
        .diagnostics
        .iter()
        .map(|d| (d.severity, d.message.clone()))
        .collect();
    assert!(
        msgs.contains(&(Severity::Error, "bw.4 isn't in the original".into())),
        "{msgs:#?}"
    );
    assert!(
        msgs.iter()
            .any(|(s, m)| *s == Severity::Warning && m.starts_with("the translation skips bw.1")),
        "{msgs:#?}"
    );
}

#[test]
fn a_book_can_hold_several_works() {
    let toml = "id = \"edda\"\ntitle = \"Edda\"\nlang = \"eng\"\n[refs]\nunit = \"stanza.line\"\nworks = { vsp = \"Völuspá\", hav = \"Hávamál\" }\n";
    let b = book(&[
        ("book.toml", toml),
        (
            "text/01.dj",
            "{#ch01-0001 ref=\"vsp.1\"}\n::: stanza\nHearing I ask\n:::\n\n{#ch02-0001 ref=\"hav.1\"}\n::: stanza\nThe gateways all\n:::\n\n{#ch03-0001 ref=\"grm.1\"}\n::: stanza\nHot art thou\n:::\n",
        ),
    ]);
    let refs: Vec<_> = verse_lines(&b)
        .into_iter()
        .filter_map(|l| l.reference)
        .collect();
    assert_eq!(refs, ["vsp.1.1", "hav.1.1"]);
    assert!(
        has(
            &b,
            Severity::Error,
            "`ref=grm.1` should be a work and a stanza number"
        ),
        "{:#?}",
        b.diagnostics
    );
}

#[test]
fn counted_verse_can_sit_under_a_book_or_canto() {
    let toml = "id = \"od\"\ntitle = \"Odyssey\"\nlang = \"eng\"\n[refs]\nprefix = \"od\"\nunit = \"line\"\n";
    let b = book(&[
        ("book.toml", toml),
        ("text/01.dj", "{#ch01-0001 ref=\"od.1\" start=1}\n::: verse\nTell me about a complicated man.\nMuse, tell me how he wandered\n:::\n\n{#ch01-0002 ref=\"od.x\" start=1}\n::: verse\nBad\n:::\n"),
    ]);
    let refs: Vec<_> = verse_lines(&b)
        .into_iter()
        .filter_map(|l| l.reference)
        .collect();
    assert_eq!(refs, ["od.1.1", "od.1.2"]);
    assert!(
        has(&b, Severity::Error, "not `od.x`"),
        "{:#?}",
        b.diagnostics
    );
}

#[test]
fn a_scheme_has_one_prefix_or_several_works() {
    let toml = "id = \"x\"\ntitle = \"X\"\nlang = \"eng\"\n[refs]\nprefix = \"bw\"\nworks = { vsp = \"Völuspá\" }\n";
    let b = book(&[("book.toml", toml), ("text/01.dj", "{#ch01-0001}\nText.\n")]);
    assert!(has(&b, Severity::Error, "either `prefix`"));
}
