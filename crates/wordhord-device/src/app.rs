//! The reader, whatever it runs on: opens the book read last, draws its
//! pages, and does what each gesture asks until it's closed.

use std::path::Path;
use std::time::Instant;

use wordhord_typeset::fonts::{Face, FontFile, Fonts};
use wordhord_typeset::page::{Page, Run};
use wordhord_typeset::render::{self, Renderer};

use crate::book::{self, Book};
use crate::platform::Platform;
use crate::reader::{Effect, Reader};
use crate::touch::Gesture;

pub fn run(screen: &mut dyn Platform, library: &Path, app: &Path) -> Result<(), String> {
    let fonts = app.join("fonts");
    let (width, height) = screen.size();
    let started = Instant::now();
    let opened = book::choose(library)
        .ok_or_else(|| format!("no books in {}", library.display()))
        .and_then(|path| Book::open(&path, app));
    let book = match opened {
        Ok(b) => b,
        Err(e) => {
            say(
                screen,
                &fonts,
                &[
                    "Wordhord can't open a book.",
                    &e,
                    "Hold a finger down to go back.",
                ],
            );
            wait_to_close(screen);
            return Err(e);
        }
    };
    eprintln!(
        "{}: {} chapters, opened in {:?}",
        book.name,
        book.chapters(),
        started.elapsed()
    );
    let _ = std::fs::write(library.join("last"), book.name.as_bytes());
    let (mut chapter, mut page) = book.position();
    let mut flash = true;
    loop {
        let t = match book.chapter(chapter) {
            Ok(t) => t,
            Err(e) => {
                say(
                    screen,
                    &fonts,
                    &[
                        "Wordhord can't open this chapter.",
                        &e,
                        "Hold a finger down to go back.",
                    ],
                );
                wait_to_close(screen);
                return Err(e);
            }
        };
        if (t.width, t.height) != (width, height) {
            let e = format!(
                "These pages are set for a {}×{} screen. This one is {width}×{height}.",
                t.width, t.height
            );
            say(screen, &fonts, &[&e, "Hold a finger down to go back."]);
            wait_to_close(screen);
            return Err(e);
        }
        let read = |src: &str| book.image(src);
        let mut renderer = Renderer::new(&book.fonts).with_images(&read);
        let mut reader = Reader::new(&t, page);
        book.remember(chapter, reader.page);
        // The page as drawn, to put back where the panel was.
        let mut shown = draw(screen, &mut renderer, &t.pages[reader.page], flash);
        flash = false;
        let step = loop {
            let gesture = screen.next();
            let effect = reader.act(gesture);
            eprintln!("{gesture:?}: {effect:?}");
            match effect {
                Effect::Nothing => {}
                Effect::Close => break None,
                // At the book's first or last page, there's nowhere to go.
                Effect::Chapter(step)
                    if (step > 0 && chapter + 1 < book.chapters()) || (step < 0 && chapter > 0) =>
                {
                    break Some(step)
                }
                Effect::Chapter(_) => {}
                Effect::Page { flash } => {
                    shown = draw(screen, &mut renderer, &t.pages[reader.page], flash);
                    book.remember(chapter, reader.page);
                }
                Effect::Panel => {
                    let Some(open) = reader.open else { continue };
                    let area = &t.panel;
                    let (w, h) = (area.width as u32, area.height as u32);
                    let page = &t.notes[open.note].pages[open.page];
                    let pixels = render::panel(&mut renderer, page, w, h);
                    screen.draw(&pixels, w, h, area.x as u32, area.y as u32, false);
                }
                Effect::ClosePanel => {
                    let top = t.panel.y as u32;
                    let rows = &shown[top as usize * width as usize..];
                    screen.draw(rows, width, height - top, 0, top, false);
                }
            }
        };
        match step {
            None => {
                book.remember(chapter, reader.page);
                return Ok(());
            }
            Some(step) if step > 0 => {
                chapter += 1;
                page = 0;
            }
            Some(_) => {
                chapter -= 1;
                // The previous chapter opens at its last page.
                page = usize::MAX;
            }
        }
    }
}

/// Draws a whole page; returns its pixels.
fn draw(screen: &mut dyn Platform, renderer: &mut Renderer, page: &Page, flash: bool) -> Vec<u8> {
    let (width, height) = screen.size();
    let started = Instant::now();
    let pixels = renderer.page(page, width, height);
    let rendered = started.elapsed();
    screen.draw(&pixels, width, height, 0, 0, flash);
    eprintln!(
        "page: rendered in {rendered:?}, shown in {:?}",
        started.elapsed() - rendered
    );
    pixels
}

pub fn wait_to_close(screen: &mut dyn Platform) {
    while !matches!(screen.next(), Gesture::Hold | Gesture::Power) {}
}

/// A few lines of text, centred on a white screen: for when there's no book
/// to show.
pub fn say(screen: &mut dyn Platform, fonts: &Path, lines: &[&str]) {
    let Some(fonts) = message_font(fonts) else {
        eprintln!("no font to show a message with");
        return;
    };
    let (width, height) = screen.size();
    let size = width as f32 / 40.0;
    let mut page = Page::default();
    let mut y = height as f32 / 3.0;
    for line in lines {
        let shaped = fonts.shape(line, Face::Regular, size, &[]);
        let w: f32 = shaped.iter().map(|s| s.width).sum();
        let mut x = ((width as f32 - w) / 2.0).max(size);
        for s in shaped {
            page.runs.push(Run {
                font: s.font,
                size: s.size,
                x,
                y,
                glyphs: s.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
            });
            x += s.width;
        }
        y += size * 2.0;
    }
    let mut renderer = Renderer::new(&fonts);
    draw(screen, &mut renderer, &page, true);
}

fn message_font(dir: &Path) -> Option<Fonts> {
    let path = dir.join("Junicode-Regular.otf");
    let data = std::fs::read(&path).ok()?;
    Fonts::from_files(vec![FontFile {
        path: path.display().to_string(),
        sha256: String::new(),
        data: Box::leak(data.into_boxed_slice()),
    }])
    .ok()
}
