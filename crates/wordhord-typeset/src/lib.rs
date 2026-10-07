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

/// Typesets one chapter of a book (1-based) for a profile: its pages in
/// `out/pages.json`, and with `previews`, a PNG of each page and note.
/// With a `translation`, each stanza's number opens the same stanza in it.
pub fn typeset_chapter(
    book: &Book,
    chapter: usize,
    profile: &profile::Profile,
    out: &Path,
    previews: bool,
    translation: Option<&Book>,
) -> Result<page::Typeset, String> {
    let ch = book
        .chapters
        .get(chapter.wrapping_sub(1))
        .ok_or_else(|| format!("{} has {} chapters", book.meta.id, book.chapters.len()))?;
    let translations: Vec<Vec<wordhord_book::model::Block>> = translation
        .map(|t| t.chapters.iter().map(|c| c.blocks.clone()).collect())
        .unwrap_or_default();
    typeset_blocks(
        &ch.blocks,
        &book.meta.lang,
        profile,
        out,
        previews,
        &translations,
        None,
    )
}

/// Lays out a chapter's blocks, in a language (ISO 639: `eng`, `en`,
/// `en-GB`), with fonts already loaded from the profile. Nothing is
/// written: this is what the device runs. `translation` is a parallel
/// book's chapters, or nothing; with `images`, which reads an image's file
/// by its source, images are laid out.
pub fn lay_out(
    blocks: &[wordhord_book::model::Block],
    lang: &str,
    profile: &profile::Profile,
    fonts: &fonts::Fonts,
    translation: &[Vec<wordhord_book::model::Block>],
    images: Option<&render::ReadImage>,
) -> page::Typeset {
    let sizes = |src: &str| {
        images
            .and_then(|read| read(src))
            .and_then(|b| render::image_size(&b))
    };
    let mut layout = layout::Layout::new(fonts, profile, lang);
    if images.is_some() {
        layout = layout.with_images(&sizes);
    }
    let translated = if translation.is_empty() {
        Vec::new()
    } else {
        let chapters: Vec<&[wordhord_book::model::Block]> =
            translation.iter().map(Vec::as_slice).collect();
        layout::translations(blocks, &chapters)
    };
    layout.translate(&translated);
    let units = layout.chapter(blocks);
    // The reference of the line each marker is first on.
    let mut on: HashMap<&str, Option<String>> = HashMap::new();
    for line in units.iter().flat_map(|u| &u.lines) {
        for (id, _, _) in line.notes.iter().chain(&line.margin_links) {
            on.entry(id).or_insert_with(|| line.reference.clone());
        }
    }
    let pages = layout.paginate(&units);
    let mut marker: HashMap<&str, usize> = HashMap::new();
    for (n, p) in pages.iter().enumerate() {
        for l in &p.links {
            if let page::Target::Note { id } = &l.to {
                marker.entry(id).or_insert(n);
            }
        }
    }
    let notes: Vec<page::Note> = layout::glosses(blocks)
        .iter()
        .chain(&translated)
        .map(|g| page::Note {
            id: g.id.clone(),
            kind: match g.kind {
                layout::GlossKind::Note => "note",
                layout::GlossKind::Commentary => "commentary",
                layout::GlossKind::Translation => "translation",
            }
            .to_string(),
            label: g.label.clone(),
            title: g.title.clone(),
            on: on.get(g.id.as_str()).cloned().flatten(),
            page: marker.get(g.id.as_str()).copied(),
            pages: layout.panel_pages(g),
        })
        .collect();
    let panel = layout.panel();

    page::Typeset {
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
    }
}

/// Typesets a chapter's blocks as `lay_out` does, into `out/pages.json`,
/// with the images used copied to `out/images/` and, with `previews`, a PNG
/// of each page and note.
pub fn typeset_blocks(
    blocks: &[wordhord_book::model::Block],
    lang: &str,
    profile: &profile::Profile,
    out: &Path,
    previews: bool,
    translation: &[Vec<wordhord_book::model::Block>],
    images: Option<&render::ReadImage>,
) -> Result<page::Typeset, String> {
    let fonts = fonts::Fonts::load(&profile.dir, &profile.fonts)?;
    let typeset = lay_out(blocks, lang, profile, &fonts, translation, images);
    let (pages, notes, panel) = (&typeset.pages, &typeset.notes, &typeset.panel);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    // The images the pages use go with them.
    if let Some(read) = images {
        let mut used: Vec<&str> = pages
            .iter()
            .flat_map(|p| &p.images)
            .map(|i| i.src.as_str())
            .collect();
        used.sort_unstable();
        used.dedup();
        for src in used {
            if let Some(bytes) = read(src) {
                let path = out.join("images").join(src);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
    }
    if previews {
        std::fs::create_dir_all(out.join("notes")).map_err(|e| e.to_string())?;
        let mut renderer = render::Renderer::new(&fonts);
        if let Some(read) = images {
            renderer = renderer.with_images(read);
        }
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
        // Each note's panel pages, and the first of each kind opened over its
        // page.
        let height = panel.height as u32;
        let mut shown = std::collections::HashSet::new();
        for note in notes {
            let name: String = note
                .id
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
                if n == 0 && shown.insert(note.kind.as_str()) {
                    if let Some(page) = note.page {
                        let mut over = rendered[page].clone();
                        let start = (panel.y as usize) * profile.width as usize;
                        over[start..start + pixels.len()].copy_from_slice(&pixels);
                        render::write_png(
                            &out.join(format!("panel-{}.png", note.kind)),
                            &over,
                            profile.width,
                            profile.height,
                        )?;
                    }
                }
            }
        }
    }
    let json = serde_json::to_string(&typeset).map_err(|e| e.to_string())?;
    std::fs::write(out.join("pages.json"), json).map_err(|e| e.to_string())?;
    Ok(typeset)
}
