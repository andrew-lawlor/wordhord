//! Typesets a Wordhord book into pages of glyph runs (SPEC.md §6–7), with
//! a PNG preview.

pub mod fonts;
pub mod layout;
pub mod linebreak;
pub mod page;
pub mod profile;
pub mod render;

use std::path::Path;

use wordhord_book::Book;

/// Typesets one chapter of a book (1-based) for a profile: its pages, and
/// a PNG of each in `out`.
pub fn typeset_chapter(
    book: &Book,
    chapter: usize,
    profile: &profile::Profile,
    out: &Path,
) -> Result<page::Typeset, String> {
    let ch = book
        .chapters
        .get(chapter.wrapping_sub(1))
        .ok_or_else(|| format!("{} has {} chapters", book.meta.id, book.chapters.len()))?;
    let f = &profile.fonts;
    let fonts = fonts::Fonts::load(
        &profile.dir,
        &f.regular,
        &f.italic,
        &f.bold,
        &f.bold_italic,
        f.fallback.as_deref(),
    )?;
    let layout = layout::Layout::new(&fonts, profile, &book.meta.lang);
    let units = layout.chapter(&ch.blocks);
    let pages = layout.paginate(&units);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut renderer = render::Renderer::new(&fonts);
    for (n, p) in pages.iter().enumerate() {
        let pixels = renderer.page(p, profile.width, profile.height);
        render::write_png(
            &out.join(format!("page-{:03}.png", n + 1)),
            &pixels,
            profile.width,
            profile.height,
        )?;
    }
    let typeset = page::Typeset {
        profile: profile.name.clone(),
        width: profile.width,
        height: profile.height,
        fonts: fonts
            .files
            .iter()
            .map(|f| page::FontRef {
                path: f.path.clone(),
                sha256: f.sha256.clone(),
            })
            .collect(),
        pages,
    };
    let json = serde_json::to_string(&typeset).map_err(|e| e.to_string())?;
    std::fs::write(out.join("pages.json"), json).map_err(|e| e.to_string())?;
    Ok(typeset)
}
