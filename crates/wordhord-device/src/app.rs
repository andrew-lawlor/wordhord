//! Wordhord, whatever it runs on: the library, and the book open in it.
//!
//! It opens the book read last, if it's still there, else the library.
//! From a book, a tap at the top brings up a bar: back to the library, or
//! to the contents. Holding a finger on a word looks it up. Wordhord
//! closes from the foot of the library's home.

use std::path::Path;
use std::thread::JoinHandle;
use std::time::Instant;

use wordhord_typeset::fonts::{Face, FontFile, Fonts};
use wordhord_typeset::page::{Page, Run};
use wordhord_typeset::profile::Profile;
use wordhord_typeset::render::{self, Renderer};

use crate::book::Book;
use crate::lexicon::{self, Lexicon};
use crate::library::{self, Entry, Library};
use crate::platform::Platform;
use crate::reader::{Effect, Reader};
use crate::touch::Gesture;
use crate::ui::{Action, Part, Recent, Rect, Row, Screen, Shelf, Ui, COVER};

/// How a book was left.
enum Leave {
    Library,
    Close,
}

pub fn run(screen: &mut dyn Platform, root: &Path, app: &Path) -> Result<(), String> {
    let fonts_dir = app.join("fonts");
    let (width, height) = screen.size();
    let started = Instant::now();
    let setup = Profile::load(&app.join("profile.toml"))
        .and_then(|p| Fonts::load(&p.dir, &p.fonts).map(|f| (p, f)));
    let (profile, fonts) = match setup {
        Ok(s) => s,
        Err(e) => {
            say(
                screen,
                &fonts_dir,
                &["Wordhord can't start.", &e, "Tap to go back."],
            );
            wait_to_close(screen);
            return Err(e);
        }
    };
    let ui = Ui::new(&fonts, width, height, profile.ppi);
    let index = app.join("library.json");
    // The first scan opens every book: say so, rather than show nothing.
    if !index.exists() {
        say(screen, &fonts_dir, &["Finding your books…"]);
    }
    let library = Library::scan(root, &index);
    let lexicon = Lexicon::open(&app.join("dictionaries"));
    eprintln!("ready in {:?}", started.elapsed());
    let mut open = library::recent(app)
        .into_iter()
        .map(|o| o.path)
        .find(|p| library.books.iter().any(|b| &b.path == p));
    let mut browser = Browser::default();
    loop {
        if let Some(path) = open.take() {
            let tools = Tools {
                ui: &ui,
                fonts: &fonts,
                profile: &profile,
                lexicon: &lexicon,
            };
            match read(screen, root, &path, app, &tools) {
                Ok(Leave::Library) => {}
                Ok(Leave::Close) => return Ok(()),
                Err(e) => {
                    eprintln!("{path}: {e}");
                    say(
                        screen,
                        &fonts_dir,
                        &["Wordhord can't open this book.", &e, "Tap to go back."],
                    );
                    screen.next();
                }
            }
        }
        match browse(screen, &mut browser, &library, root, app, &ui, &fonts) {
            Some(path) => open = Some(path),
            None => return Ok(()),
        }
    }
}

// ---- the library -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Home,
    Shelf(Shelf),
    Group(Shelf, usize),
}

/// Where in the library the reader is: the views gone through, each with
/// its page.
#[derive(Default)]
struct Browser {
    stack: Vec<(View, usize)>,
}

impl Browser {
    fn current(&self) -> (View, usize) {
        self.stack.last().copied().unwrap_or((View::Home, 0))
    }

    fn go(&mut self, action: &Action) {
        match action {
            Action::Shelf(s) => self.stack.push((View::Shelf(*s), 0)),
            Action::Group(s, k) => self.stack.push((View::Group(*s, *k), 0)),
            Action::Back => {
                self.stack.pop();
            }
            _ => {}
        }
    }

    fn turn(&mut self, by: isize, pages: usize) {
        let (view, page) = self.current();
        let to = page as isize + by;
        if to >= 0 && (to as usize) < pages {
            self.stack.pop();
            self.stack.push((view, to as usize));
        }
    }

    fn screen(&self, ui: &Ui, library: &Library, recent: &[library::Opened]) -> Screen {
        let progress = |path: &str| recent.iter().find(|o| o.path == path).map(|o| o.progress);
        let row = |e: &Entry| Row {
            title: e.title.clone(),
            detail: {
                let (author, series) = e.byline();
                let mut parts = vec![Part::Caps(author)];
                if let Some(s) = series {
                    parts.push(Part::Plain("  ·  ".into()));
                    parts.push(Part::Italic(s));
                }
                parts
            },
            cover: Some((e.path.clone(), e.cover_size.unwrap_or((0, 0)))),
            progress: progress(&e.path),
            count: None,
            mark: false,
            action: Action::Open(e.path.clone()),
            depth: 0,
        };
        let groups = |shelf: Shelf| match shelf {
            Shelf::Authors => library.by_author(),
            Shelf::Series => library.by_series(),
            Shelf::Tags => library.by_tag(),
            Shelf::All => Vec::new(),
        };
        let (view, page) = self.current();
        match view {
            View::Home => {
                let recent: Vec<Recent> = recent
                    .iter()
                    .filter_map(|o| {
                        let e = library.books.iter().find(|b| b.path == o.path)?;
                        Some(Recent {
                            path: e.path.clone(),
                            title: e.title.clone(),
                            cover: e.cover_size,
                            progress: o.progress,
                        })
                    })
                    .take(4)
                    .collect();
                ui.home(
                    &recent,
                    [
                        (Shelf::Authors, library.by_author().len()),
                        (Shelf::Series, library.by_series().len()),
                        (Shelf::Tags, library.by_tag().len()),
                        (Shelf::All, library.books.len()),
                    ],
                )
            }
            View::Shelf(Shelf::All) => {
                let rows: Vec<Row> = library.books.iter().map(row).collect();
                ui.list(Shelf::All.name(), &rows, page, true)
            }
            View::Shelf(shelf) => {
                let rows: Vec<Row> = groups(shelf)
                    .iter()
                    .enumerate()
                    .map(|(k, (name, books))| Row {
                        title: name.clone(),
                        detail: match shelf {
                            Shelf::Series => books
                                .first()
                                .and_then(|b| b.authors.first().cloned())
                                .map(Part::Caps)
                                .into_iter()
                                .collect(),
                            _ => Vec::new(),
                        },
                        cover: None,
                        progress: None,
                        count: Some(books.len()),
                        mark: false,
                        action: Action::Group(shelf, k),
                        depth: 0,
                    })
                    .collect();
                ui.list(shelf.name(), &rows, page, true)
            }
            View::Group(shelf, k) => {
                let groups = groups(shelf);
                let Some((name, books)) = groups.get(k) else {
                    return ui.list(shelf.name(), &[], 0, true);
                };
                let rows: Vec<Row> = books.iter().map(|e| row(e)).collect();
                ui.list(name, &rows, page, true)
            }
        }
    }
}

/// The library, until a book is chosen (its path) or Wordhord is closed.
fn browse(
    screen: &mut dyn Platform,
    browser: &mut Browser,
    library: &Library,
    root: &Path,
    app: &Path,
    ui: &Ui,
    fonts: &Fonts,
) -> Option<String> {
    let (width, height) = screen.size();
    let read = |src: &str| {
        let path = src.strip_prefix(COVER)?;
        library
            .books
            .iter()
            .find(|b| b.path == path)?
            .cover_bytes(root)
    };
    let mut renderer = Renderer::new(fonts)
        .with_images(&read)
        .with_picture_cache(app.join("covers"));
    let mut flash = true;
    loop {
        let recent = library::recent(app);
        let s = browser.screen(ui, library, &recent);
        let started = Instant::now();
        let pixels = renderer.page(&s.page, width, height);
        screen.draw(&pixels, width, height, 0, 0, flash);
        eprintln!(
            "library: {:?} drawn in {:?}",
            browser.current(),
            started.elapsed()
        );
        flash = false;
        loop {
            match screen.next() {
                Gesture::Power => return None,
                Gesture::SwipeLeft if browser.current().1 + 1 < s.pages => {
                    browser.turn(1, s.pages);
                    break;
                }
                Gesture::SwipeRight if browser.current().1 > 0 => {
                    browser.turn(-1, s.pages);
                    break;
                }
                Gesture::Tap(x, y) => match s.hit(x as f32, y as f32) {
                    Some(Action::Open(path)) => return Some(path.clone()),
                    Some(Action::Close) => return None,
                    Some(a) => {
                        let a = a.clone();
                        // Back from home has nowhere to go.
                        if !(a == Action::Back && browser.stack.is_empty()) {
                            browser.go(&a);
                            flash = true;
                            break;
                        }
                    }
                    None => {}
                },
                _ => {}
            }
        }
    }
}

// ---- a book ------------------------------------------------------------------

/// What reading needs besides the book.
struct Tools<'a> {
    ui: &'a Ui<'a>,
    fonts: &'a Fonts,
    profile: &'a Profile,
    lexicon: &'a Lexicon,
}

/// Where reading goes from a chapter.
enum Step {
    Next,
    Previous,
    Jump(usize),
    Leave(Leave),
}

fn read(
    screen: &mut dyn Platform,
    root: &Path,
    path: &str,
    app: &Path,
    tools: &Tools,
) -> Result<Leave, String> {
    let (ui, fonts) = (tools.ui, tools.fonts);
    let (width, height) = screen.size();
    let started = Instant::now();
    let book = Book::open(&root.join(path), app)?;
    eprintln!(
        "{}: {} chapters, opened in {:?}",
        book.name,
        book.chapters(),
        started.elapsed()
    );
    let (mut chapter, mut page) = book.position();
    let mut flash = true;
    // The chapter being laid out ahead, on another thread.
    let mut ahead: Option<(usize, JoinHandle<()>)> = None;
    // A renderer for the bar and the contents, in the app's own fonts.
    let mut chrome = Renderer::new(fonts);
    loop {
        // If this chapter is still being laid out, wait for it rather than
        // do the work twice.
        if let Some((k, handle)) = ahead.take() {
            if k == chapter && !handle.is_finished() {
                let waited = Instant::now();
                let _ = handle.join();
                eprintln!("waited {:?} for chapter {k}", waited.elapsed());
            } else if !handle.is_finished() {
                ahead = Some((k, handle));
            }
        }
        let t = book.chapter(chapter)?;
        if (t.width, t.height) != (width, height) {
            return Err(format!(
                "These pages are set for a {}×{} screen. This one is {width}×{height}.",
                t.width, t.height
            ));
        }
        let read = |src: &str| book.image(src);
        let mut renderer = Renderer::new(&book.fonts)
            .with_images(&read)
            .with_picture_cache(book.picture_dir());
        let mut reader = Reader::new(&t, page);
        let chapters = book.chapters() as f32;
        let progress =
            |page: usize| (chapter as f32 + (page + 1) as f32 / t.pages.len() as f32) / chapters;
        book.remember(chapter, reader.page);
        library::opened(app, path, progress(reader.page));
        // The page as drawn, to put back where the panel or bar was.
        let mut shown = draw(screen, &mut renderer, &t.pages[reader.page], flash);
        flash = false;
        if ahead.is_none() {
            ahead = Some((chapter + 1, lay_out_ahead(&book, chapter)));
        }
        let step = 'reading: loop {
            let gesture = screen.next();
            let effect = reader.act(gesture);
            eprintln!("{gesture:?}: {effect:?}");
            match effect {
                Effect::Nothing => {}
                Effect::Close => break Step::Leave(Leave::Close),
                // At the book's first or last page, there's nowhere to go.
                Effect::Chapter(step)
                    if (step > 0 && chapter + 1 < book.chapters()) || (step < 0 && chapter > 0) =>
                {
                    break if step > 0 { Step::Next } else { Step::Previous };
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
                Effect::Look(k) => {
                    let words = &t.pages[reader.page].words;
                    // A word hyphenated across lines is one word.
                    let mut parts = vec![k];
                    let mut word = words[k].text.clone();
                    if words[k].continues && k > 0 {
                        word = format!("{}{word}", words[k - 1].text);
                        parts.insert(0, k - 1);
                    }
                    if words.get(k + 1).is_some_and(|w| w.continues) {
                        word.push_str(&words[k + 1].text);
                        parts.push(k + 1);
                    }
                    // Mark the word at once, before the panel comes.
                    let boxes: Vec<Rect> = parts
                        .iter()
                        .map(|&n| {
                            let w = &words[n];
                            let (x0, x1) = w.letters.unwrap_or((w.x0, w.x1));
                            Rect {
                                x0,
                                y0: w.y0,
                                x1,
                                y1: w.y1,
                            }
                        })
                        .collect();
                    let marked = highlight(&shown, width, height, &boxes);
                    for b in &boxes {
                        let (y0, y1) = (b.y0.max(0.0) as u32, (b.y1 as u32).min(height));
                        let rows = &marked[(y0 * width) as usize..(y1 * width) as usize];
                        screen.draw(rows, width, y1 - y0, 0, y0, false);
                    }
                    // Looked up as printed, so a capital can say it's a name;
                    // shown as the dictionary keys it.
                    let shown_as = wordhord_dict::key(&word);
                    let looked = Instant::now();
                    let lang = book.language();
                    let blocks = match tools.lexicon.look_up(&word, &lang) {
                        Some((_, readings)) => lexicon::entry_blocks(&shown_as, &readings),
                        None => lexicon::not_found(&shown_as, &lang, !tools.lexicon.is_empty()),
                    };
                    let word = shown_as;
                    let (mut area, pages) =
                        wordhord_typeset::panel_pages(&blocks, &lang, tools.profile, fonts);
                    // Under the panel, the word would be hidden: the panel
                    // comes down from the top instead.
                    let above = boxes.iter().any(|b| b.y1 > area.y);
                    if above {
                        area.y = 0.0;
                        // Down to the end of any line it would cut through,
                        // so no slivers of letters show under it.
                        let word_top = boxes.iter().map(|b| b.y0).fold(f32::MAX, f32::min);
                        let cut = words
                            .iter()
                            .filter(|w| w.y0 < area.height && w.y1 > area.height)
                            .map(|w| w.y1)
                            .fold(area.height, f32::max);
                        if cut <= word_top {
                            area.height = cut.ceil();
                        }
                    }
                    eprintln!(
                        "look up {word:?}: {} panel pages in {:?}",
                        pages.len(),
                        looked.elapsed()
                    );
                    let (w, h) = (area.width as u32, area.height as u32);
                    let mut at = 0;
                    loop {
                        let mut pixels = render::panel(&mut chrome, &pages[at], w, h);
                        if above {
                            // The rule goes on the panel's lower edge.
                            let rule = (w * 3) as usize;
                            pixels[..rule].fill(255);
                            let n = pixels.len();
                            pixels[n - rule..].fill(0);
                        }
                        screen.draw(&pixels, w, h, area.x as u32, area.y as u32, false);
                        // In the panel, the right half pages on and the left
                        // back; anything else closes it.
                        let inside = |y: i32| (area.y..area.y + area.height).contains(&(y as f32));
                        let next = loop {
                            match screen.next() {
                                Gesture::Tap(x, y) if inside(y) => {
                                    if x as f32 >= area.width / 2.0 && at + 1 < pages.len() {
                                        break at + 1;
                                    }
                                    if (x as f32) < area.width / 2.0 && at > 0 {
                                        break at - 1;
                                    }
                                }
                                Gesture::Power => break 'reading Step::Leave(Leave::Close),
                                _ => break usize::MAX,
                            }
                        };
                        if next == usize::MAX {
                            break;
                        }
                        at = next;
                    }
                    // The page as it was: no panel, no mark.
                    let top = (area.y as u32)
                        .min(boxes.iter().map(|b| b.y0 as u32).min().unwrap_or(height));
                    let bottom = ((area.y + area.height) as u32)
                        .max(boxes.iter().map(|b| b.y1 as u32).max().unwrap_or(0))
                        .min(height);
                    let rows = &shown[(top * width) as usize..(bottom * width) as usize];
                    screen.draw(rows, width, bottom - top, 0, top, false);
                }
                Effect::Menu => {
                    let (bar, bar_h) = ui.reading_bar(&book.title());
                    let pixels = chrome.page(&bar.page, width, bar_h);
                    screen.draw(&pixels, width, bar_h, 0, 0, false);
                    let action = match screen.next() {
                        Gesture::Tap(x, y) => bar.hit(x as f32, y as f32).cloned(),
                        Gesture::Power => break Step::Leave(Leave::Close),
                        _ => None,
                    };
                    match action {
                        Some(Action::Library) => break Step::Leave(Leave::Library),
                        Some(Action::Contents) => {
                            match contents(screen, &book, ui, &mut chrome, chapter) {
                                Some(Ok(k)) => break Step::Jump(k),
                                Some(Err(leave)) => break Step::Leave(leave),
                                None => {
                                    shown = draw(screen, &mut renderer, &t.pages[reader.page], true)
                                }
                            }
                        }
                        // Anything else puts the page back.
                        _ => screen.draw(
                            &shown[..(bar_h * width) as usize],
                            width,
                            bar_h,
                            0,
                            0,
                            false,
                        ),
                    }
                }
            }
        };
        library::opened(app, path, progress(reader.page));
        match step {
            Step::Leave(leave) => {
                book.remember(chapter, reader.page);
                return Ok(leave);
            }
            Step::Next => {
                chapter += 1;
                page = 0;
            }
            Step::Previous => {
                chapter -= 1;
                // The previous chapter opens at its last page.
                page = usize::MAX;
            }
            Step::Jump(k) => {
                chapter = k;
                page = 0;
                flash = true;
            }
        }
    }
}

/// The book's contents. A chapter chosen, `Ok`; Wordhord closed, `Err`;
/// back to the page, `None`.
fn contents(
    screen: &mut dyn Platform,
    book: &Book,
    ui: &Ui,
    chrome: &mut Renderer,
    current: usize,
) -> Option<Result<usize, Leave>> {
    let (width, height) = screen.size();
    let entries = book.contents();
    // The entry for the chapter being read: the last one at or before it.
    let at = entries
        .iter()
        .rposition(|(_, _, c)| *c <= current)
        .unwrap_or(0);
    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(k, (label, depth, chapter))| Row {
            title: label.clone(),
            detail: Vec::new(),
            cover: None,
            progress: None,
            count: None,
            mark: k == at,
            action: Action::Chapter(*chapter),
            depth: *depth,
        })
        .collect();
    let per_page = ui
        .list("Contents", &rows, 0, true)
        .hits
        .len()
        .saturating_sub(1)
        .max(1);
    let mut page = at / per_page;
    loop {
        let s = ui.list("Contents", &rows, page, true);
        let pixels = chrome.page(&s.page, width, height);
        screen.draw(&pixels, width, height, 0, 0, true);
        loop {
            match screen.next() {
                Gesture::Power => return Some(Err(Leave::Close)),
                Gesture::SwipeLeft if page + 1 < s.pages => {
                    page += 1;
                    break;
                }
                Gesture::SwipeRight if page > 0 => {
                    page -= 1;
                    break;
                }
                Gesture::Tap(x, y) => match s.hit(x as f32, y as f32) {
                    Some(Action::Chapter(k)) => return Some(Ok(*k)),
                    Some(Action::Back) => return None,
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

/// On another thread, readies the images on the rest of chapter `now`,
/// then lays out the next chapter and readies its images too, so they're
/// waiting in the cache when the reader gets there. The thread opens its
/// own copy of the book and runs at a lower priority than the reader.
fn lay_out_ahead(book: &Book, now: usize) -> JoinHandle<()> {
    let (path, app) = (book.path.clone(), book.app.clone());
    let next = now + 1;
    let wanted = next < book.chapters() && !book.is_cached(next);
    std::thread::spawn(move || {
        // On Linux, nice applies to the calling thread alone.
        unsafe {
            libc::nice(10);
        }
        let started = Instant::now();
        let Ok(b) = Book::open(&path, &app) else {
            return;
        };
        if let Ok(t) = b.chapter(now) {
            b.prepare_pictures(&t);
        }
        if wanted {
            match b.chapter(next) {
                Ok(t) => {
                    b.prepare_pictures(&t);
                    eprintln!("ahead: chapter {next} ready in {:?}", started.elapsed());
                }
                Err(e) => eprintln!("ahead: chapter {next}: {e}"),
            }
        }
    })
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
    while !matches!(screen.next(), Gesture::Tap(..) | Gesture::Power) {}
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

/// The page with each box's paper turned light grey, so a word stands
/// marked while the ink stays as it was.
fn highlight(page: &[u8], width: u32, height: u32, boxes: &[Rect]) -> Vec<u8> {
    let mut out = page.to_vec();
    for b in boxes {
        // A little wider than the word, and clear of the lines around it.
        let pad = (b.y1 - b.y0) * 0.12;
        let x0 = (b.x0 - pad * 0.6).max(0.0) as u32;
        let x1 = ((b.x1 + pad * 0.6) as u32).min(width);
        let y0 = (b.y0 + pad).max(0.0) as u32;
        let y1 = ((b.y1 - pad * 0.5) as u32).min(height);
        for y in y0..y1 {
            let row = (y * width) as usize;
            for p in &mut out[row + x0 as usize..row + x1 as usize] {
                *p = (*p).min(204);
            }
        }
    }
    out
}
