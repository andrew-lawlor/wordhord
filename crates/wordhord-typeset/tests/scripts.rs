//! Scripts the faces lack, set by the fallbacks: Hebrew and Arabic right
//! to left, Devanagari with its own font.

use std::path::Path;

use wordhord_typeset::fonts::{Face, Fonts};
use wordhord_typeset::profile::Profile;

fn fonts() -> Fonts {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile = Profile::load(&root.join("profiles/elipsa-2e.toml")).unwrap();
    Fonts::load(&profile.dir, &profile.fonts).unwrap()
}

#[test]
fn hebrew_arabic_and_devanagari_find_their_fonts() {
    let fonts = fonts();
    for (word, file) in [
        ("לִוְיָתָן", "NotoSerifHebrew"),
        ("كتاب", "NotoNaskhArabic"),
        ("वीर", "NotoSerifDevanagari"),
    ] {
        let runs = fonts.shape(word, Face::Regular, 40.0, &[]);
        assert_eq!(runs.len(), 1, "{word}");
        assert!(fonts.files[runs[0].font].path.contains(file), "{word}");
        // No empty boxes: glyph 0 is the font's "missing".
        assert!(runs[0].glyphs.iter().all(|g| g.id != 0), "{word}");
    }
}

#[test]
fn a_hebrew_word_is_drawn_from_its_last_letter_on_the_left() {
    let fonts = fonts();
    let runs = fonts.shape("שלום", Face::Regular, 40.0, &[]);
    let clusters: Vec<u32> = runs[0].glyphs.iter().map(|g| g.cluster).collect();
    // Seen left to right, the word's end comes first.
    assert!(clusters.first() > clusters.last(), "{clusters:?}");
    // A Latin word around it is left to right still.
    let latin = fonts.shape("hate", Face::Regular, 40.0, &[]);
    let c: Vec<u32> = latin[0].glyphs.iter().map(|g| g.cluster).collect();
    assert!(c.windows(2).all(|w| w[0] <= w[1]));
}
