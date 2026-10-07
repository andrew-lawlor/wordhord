use std::path::Path;

use wordhord_book::{Book, Severity};
use wordhord_typeset::fonts::Fonts;
use wordhord_typeset::layout::{self, Layout};
use wordhord_typeset::page::Target;
use wordhord_typeset::profile::Profile;

const SOURCE: &str = "\
{#ch01-0001}
# Canto

{#ch01-0002}
Midway upon the journey of our life.[^2]

{#ch01-0003}
I found myself within a forest dark.

[^2]: The action begins on Good Friday.

  It ends on the first Sunday after Easter.
";

fn with_layout(test: impl Fn(&Layout, &[wordhord_book::model::Block])) {
    with_source(SOURCE, test)
}

/// A one-chapter book from Djot source, checked for errors.
fn load(source: &str) -> Book {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wordhord-typeset-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(dir.join("text")).unwrap();
    std::fs::write(
        dir.join("book.toml"),
        "id = \"t\"\ntitle = \"T\"\nlang = \"eng\"\n[refs]\nprefix = \"vsp\"\nunit = \"stanza.line\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("text/01.dj"), source).unwrap();
    let book = Book::load(&dir).expect("book loads");
    std::fs::remove_dir_all(&dir).unwrap();
    let errors: Vec<_> = book
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    book
}

fn with_source(source: &str, test: impl Fn(&Layout, &[wordhord_book::model::Block])) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile = Profile::load(&root.join("profiles/elipsa-2e.toml")).unwrap();
    let f = &profile.fonts;
    let fonts = Fonts::load(
        &profile.dir,
        &f.regular,
        &f.italic,
        &f.bold,
        &f.bold_italic,
        f.fallback.as_deref(),
    )
    .unwrap();
    let layout = Layout::new(&fonts, &profile, "eng");
    let book = load(source);
    test(&layout, &book.chapters[0].blocks);
}

#[test]
fn a_marker_is_a_link_wide_enough_to_tap() {
    with_layout(|layout, blocks| {
        let pages = layout.paginate(&layout.chapter(blocks));
        let link = pages[0]
            .links
            .iter()
            .find(|l| {
                l.to == Target::Note {
                    id: "note-2".into(),
                }
            })
            .expect("the marker links to its note");
        let reach = layout.profile.mm(6.0);
        assert!(link.x1 - link.x0 >= reach - 0.01);
    });
}

#[test]
fn a_note_in_the_chapters_notes_links_back_to_its_marker() {
    with_layout(|layout, blocks| {
        let pages = layout.paginate(&layout.chapter(blocks));
        let back: Vec<_> = pages
            .iter()
            .flat_map(|p| &p.links)
            .filter(|l| matches!(l.to, Target::Page { .. }))
            .collect();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].to, Target::Page { page: 0 });
        assert!(!pages
            .iter()
            .flat_map(|p| &p.links)
            .any(|l| matches!(l.to, Target::Marker(_))));
    });
}

#[test]
fn a_note_is_set_inside_the_panel() {
    with_layout(|layout, blocks| {
        let notes = layout::glosses(blocks);
        assert_eq!(notes.len(), 1);
        let pages = layout.panel_pages(&notes[0]);
        assert_eq!(pages.len(), 1);
        let panel = layout.panel();
        let words = &pages[0].words;
        assert_eq!(words[0].text, "2.");
        assert!(words
            .iter()
            .all(|w| w.y1 < panel.height && w.x1 < panel.width));
        // The second paragraph is indented.
        let it = words.iter().find(|w| w.text == "It").unwrap();
        assert!(it.x0 > words[0].x0 + 10.0);
    });
}

const STANZAS: &str = "\
{#ch01-0001 ref=\"vsp.1\"}
::: stanza
Hearing I ask from the holy races,
From Heimdall's sons, both high and low.
:::

{#ch01-0002}
Then Othin spoke.

{#ch01-0003 on=\"vsp.1\"}
::::: commentary
Holy: omitted in Regius.
:::::

{#ch01-0004 on=\"#ch01-0002\"}
::::: commentary
Othin: chief of the gods.
:::::
";

#[test]
fn commentary_opens_from_a_mark_beside_its_passage() {
    with_source(STANZAS, |layout, blocks| {
        let glosses = layout::glosses(blocks);
        let titles: Vec<_> = glosses.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Stanza 1", "On \u{201c}Then Othin spoke.\u{201d}"]);
        let pages = layout.paginate(&layout.chapter(blocks));
        let to = |id: &str| {
            pages[0]
                .links
                .iter()
                .find(|l| l.to == Target::Note { id: id.into() })
                .cloned()
        };
        let stanza = to("ch01-0003").expect("a mark beside the stanza");
        let prose = to("ch01-0004").expect("a mark beside the paragraph");
        // Both marks are in the left margin.
        let left = layout.profile.mm(layout.profile.margins.left);
        assert!(stanza.x0 < left + layout.body * 2.0);
        assert!(prose.x0 < left);
        assert!(stanza.y1 <= prose.y0);
    });
}

const NORSE: &str = "\
{#ch01-0001 ref=\"vsp.1\"}
::: stanza
Hljóðs bið ek | allar kindir,
meiri ok minni, | mǫgu Heimdal[l]{.corr}ar!
:::
";

#[test]
fn an_emended_word_is_one_word() {
    with_source(NORSE, |layout, blocks| {
        let pages = layout.paginate(&layout.chapter(blocks));
        let words: Vec<_> = pages[0].words.iter().map(|w| w.text.as_str()).collect();
        assert!(words.contains(&"Heimdallar!"), "{words:?}");
    });
}

#[test]
fn second_half_lines_share_a_column() {
    with_source(NORSE, |layout, blocks| {
        let pages = layout.paginate(&layout.chapter(blocks));
        let x = |text: &str| pages[0].words.iter().find(|w| w.text == text).unwrap().x0;
        assert!((x("allar") - x("mǫgu")).abs() < 0.01);
    });
}

#[test]
fn no_word_crosses_the_right_margin() {
    // This note fits on one line only with its spaces shrunk.
    let source = "\
{#ch01-0001}
Of my renown.[^21]

[^21]: Dante seems to have been already conscious of the fame which his {_Vita Nuova_} and {_Canzoni_} had given him.
";
    with_source(source, |layout, blocks| {
        let p = layout.profile;
        let right = p.width as f32 - p.mm(p.margins.right);
        let pages = layout.paginate(&layout.chapter(blocks));
        for w in pages.iter().flat_map(|p| &p.words) {
            assert!(
                w.x1 <= right + 0.5,
                "{} ends at {} past {right}",
                w.text,
                w.x1
            );
        }
    });
}

const ENGLISH: &str = "\
{#ch01-0001 ref=\"vsp.1\"}
::: stanza
A hearing I ask from all kindreds,[^2]
greater and lesser, the sons of Heimdallr!
:::

[^2]: Pettit's note, which stays with the English.
";

#[test]
fn a_stanzas_number_opens_its_translation() {
    let english = load(ENGLISH);
    with_source(NORSE, |layout, blocks| {
        let chapters = [english.chapters[0].blocks.as_slice()];
        let translated = layout::translations(blocks, &chapters);
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[0].id, "tr-vsp.1");
        layout.translate(&translated);
        let pages = layout.paginate(&layout.chapter(blocks));
        let link = pages[0]
            .links
            .iter()
            .find(|l| {
                l.to == Target::Note {
                    id: "tr-vsp.1".into(),
                }
            })
            .expect("the stanza number is a link");
        // It's in the margin, left of the text.
        let first = pages[0].words.iter().map(|w| w.x0).fold(f32::MAX, f32::min);
        assert!(link.x0 < first);
        // The translation leaves its notes behind.
        let panel = layout.panel_pages(&translated[0]);
        let words: Vec<_> = panel[0].words.iter().map(|w| w.text.as_str()).collect();
        assert!(words.contains(&"kindreds,"), "{words:?}");
        assert!(panel[0].links.iter().all(|l| l.to
            != Target::Note {
                id: "note-2".into()
            }));
    });
}
