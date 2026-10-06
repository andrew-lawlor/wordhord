//! Typesets a Wordhord book into pages of glyph runs (SPEC.md §6–7), with
//! a PNG preview.

pub mod fonts;
pub mod layout;
pub mod linebreak;
pub mod page;
pub mod profile;
pub mod render;

use std::collections::HashMap;
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
    // The reference of the line each marker is first on.
    let mut on: HashMap<&str, Option<String>> = HashMap::new();
    for line in units.iter().flat_map(|u| &u.lines) {
        for (label, _, _) in &line.notes {
            on.entry(label).or_insert_with(|| line.reference.clone());
        }
    }
    let pages = layout.paginate(&units);
    let mut marker: HashMap<&str, usize> = HashMap::new();
    for (n, p) in pages.iter().enumerate() {
        for l in &p.links {
            if let page::Target::Note { label } = &l.to {
                marker.entry(label).or_insert(n);
            }
        }
    }
    let notes: Vec<page::Note> = layout::notes(&ch.blocks)
        .into_iter()
        .map(|(label, blocks)| page::Note {
            label: label.to_string(),
            on: on.get(label).cloned().flatten(),
            page: marker.get(label).copied(),
            pages: layout.note_pages(label, blocks),
        })
        .collect();
    let panel = layout.panel();

    std::fs::create_dir_all(out.join("notes")).map_err(|e| e.to_string())?;
    let mut renderer = render::Renderer::new(&fonts);
    let mut rendered = Vec::with_capacity(pages.len());
    for (n, p) in pages.iter().enumerate() {
        let pixels = renderer.page(p, profile.width, profile.height);
        render::write_png(
            &out.join(format!("page-{:03}.png", n + 1)),
            &pixels,
            profile.width,
            profile.height,
        )?;
        rendered.push(pixels);
    }
    // Each note's panel pages, and the first note opened over its page.
    let height = panel.height as u32;
    for (k, note) in notes.iter().enumerate() {
        let name: String = note
            .label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        for (n, p) in note.pages.iter().enumerate() {
            let pixels = render::panel(&mut renderer, p, profile.width, height);
            render::write_png(
                &out.join("notes").join(format!("{name}-{}.png", n + 1)),
                &pixels,
                profile.width,
                height,
            )?;
            if k == 0 && n == 0 {
                if let Some(page) = note.page {
                    let mut over = rendered[page].clone();
                    let start = (panel.y as usize) * profile.width as usize;
                    over[start..start + pixels.len()].copy_from_slice(&pixels);
                    render::write_png(
                        &out.join("panel-preview.png"),
                        &over,
                        profile.width,
                        profile.height,
                    )?;
                }
            }
        }
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
        panel,
        notes,
    };
    let json = serde_json::to_string(&typeset).map_err(|e| e.to_string())?;
    std::fs::write(out.join("pages.json"), json).map_err(|e| e.to_string())?;
    Ok(typeset)
}
