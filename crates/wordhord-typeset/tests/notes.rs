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
        "id = \"t\"\ntitle = \"T\"\nlang = \"eng\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("text/01.dj"), SOURCE).unwrap();
    let book = Book::load(&dir).expect("book loads");
    std::fs::remove_dir_all(&dir).unwrap();
    let errors: Vec<_> = book
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    test(&layout, &book.chapters[0].blocks);
}

#[test]
fn a_marker_is_a_link_wide_enough_to_tap() {
    with_layout(|layout, blocks| {
        let pages = layout.paginate(&layout.chapter(blocks));
        let link = pages[0]
            .links
            .iter()
            .find(|l| l.to == Target::Note { label: "2".into() })
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
        let notes = layout::notes(blocks);
        assert_eq!(notes.len(), 1);
        let pages = layout.note_pages(notes[0].0, notes[0].1);
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
