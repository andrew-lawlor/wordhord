//! Wordhord, whatever it runs on: the library, and the book open in it.
//!
//! It opens the book read last, if it's still there, else the library.
//! From a book, a tap at the top brings up a bar: back to the library, or
//! to the contents. Holding a finger on a word looks it up. Wordhord
//! closes from the foot of the library's home.

use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use wordhord_typeset::fonts::{Face, FontFile, Fonts};
use wordhord_typeset::page::{Page, Run, Typeset};
use wordhord_typeset::profile::{FontPaths, Profile};
use wordhord_typeset::render::{self, Renderer};

use crate::book::Book;
use crate::fsrs;
use crate::hoard::{self, Hoard, Kept};
use crate::lexicon::{self, Lexicon};
use crate::library::{self, Entry, Library};
use crate::light::Light;
use crate::platform::Platform;
use crate::reader::{Effect, Reader};
use crate::settings::{self, Settings};
use crate::touch::Gesture;
use crate::ui::{Action, Choice, Part, Recent, Rect, Row, Screen, Shelf, Ui, COVER};

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
    let hoard = RefCell::new(Hoard::load(app));
    // On the desktop, a stand-in for /sys in the app folder, if made.
    let light = RefCell::new(if cfg!(target_arch = "arm") {
        Light::find(
            Path::new("/sys/class"),
            Path::new("/mnt/onboard/.kobo/Kobo/Kobo eReader.conf"),
        )
    } else {
        Light::find(&app.join("sys/class"), &app.join("sys/nickel.conf"))
    });
    if let Some(l) = light.borrow().as_ref() {
        eprintln!("light: {} bright, {:?} warm", l.brightness(), l.warmth());
    }
    eprintln!("ready in {:?}", started.elapsed());
    let mut open = library::recent(app)
        .into_iter()
        .map(|o| o.path)
        .find(|p| library.books.iter().any(|b| &b.path == p));
    let mut browser = Browser::default();
    let tools = Tools {
        ui: &ui,
        fonts: &fonts,
        profile: &profile,
        lexicon: &lexicon,
        hoard: &hoard,
        light: &light,
    };
    loop {
        if let Some(path) = open.take() {
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
        match browse(screen, &mut browser, &library, root, app, &tools) {
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
    Hoard,
    /// A word of the hoard, by its place in the list.
    Word(usize),
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
            Action::Hoard => self.stack.push((View::Hoard, 0)),
            Action::Word(k) => self.stack.push((View::Word(*k), 0)),
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

    fn screen(&self, tools: &Tools, library: &Library, recent: &[library::Opened]) -> Screen {
        let ui = tools.ui;
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
                    tools.hoard.borrow().words().len(),
                    tools.hoard.borrow().due(hoard::now()).len(),
                )
            }
            View::Hoard => {
                let hoard = tools.hoard.borrow();
                let rows: Vec<Row> = hoard
                    .words()
                    .iter()
                    .enumerate()
                    .map(|(k, w)| Row {
                        title: w.head.to_string(),
                        detail: vec![
                            Part::Caps(lexicon::language_name(w.lang).to_lowercase()),
                            Part::Plain("  ·  ".into()),
                            Part::Italic(w.pos.to_string()),
                        ],
                        cover: None,
                        progress: None,
                        count: Some(w.kept.len()),
                        mark: false,
                        action: Action::Word(k),
                        depth: 0,
                    })
                    .collect();
                ui.list("The hoard", &rows, page, true)
            }
            View::Word(k) => word_page(tools, k, page),
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

fn block(kind: wordhord_book::model::BlockKind) -> wordhord_book::model::Block {
    wordhord_book::model::Block {
        id: None,
        kind,
        lang: None,
        reference: None,
        line: 0,
    }
}

/// A sentence a word was kept from, the word in italic where it was met,
/// with the book's title under it. The sentence's words carry `id`, so the
/// word can be found on the page and marked.
fn kept_quote(kept: &Kept, id: &str) -> wordhord_book::model::Block {
    use wordhord_book::model::{Block, BlockKind, Inline};
    let para = |inlines| block(BlockKind::Paragraph(inlines));
    let with_id = |b: Block| Block {
        id: Some(id.to_string()),
        ..b
    };
    let chars: Vec<char> = kept.sentence.chars().collect();
    let len = kept.word.chars().count();
    let end = kept.at + len;
    let found = end <= chars.len()
        && wordhord_dict::key(&chars[kept.at..end].iter().collect::<String>())
            == wordhord_dict::key(&kept.word);
    let sentence = if found {
        let text = |r: &[char]| Inline::Text(r.iter().collect());
        vec![
            text(&chars[..kept.at]),
            Inline::Emphasis(vec![text(&chars[kept.at..end])]),
            text(&chars[end..]),
        ]
    } else {
        vec![Inline::Text(kept.sentence.clone())]
    };
    with_id(block(BlockKind::Quote(vec![
        with_id(para(sentence)),
        para(vec![
            Inline::Text("— ".into()),
            Inline::Emphasis(vec![Inline::Text(kept.book.clone())]),
        ]),
    ])))
}

/// Where the kept words are on a page set from `kept_quote`s: the `k`th
/// quote has the id `kept-k`. Their letters, to mark grey.
fn kept_marks(page: &Page, kept: &[&Kept]) -> Vec<Rect> {
    let mut boxes = Vec::new();
    for (k, kept) in kept.iter().enumerate() {
        let id = format!("kept-{k}");
        // The word's place in the sentence, counted as the page counts.
        let before: String = kept.sentence.chars().take(kept.at).collect();
        let nth = before.split_whitespace().count();
        let words: Vec<_> = page.words.iter().filter(|w| w.block == id).collect();
        let Some(start) = words
            .iter()
            .enumerate()
            .filter(|(_, w)| !w.continues)
            .nth(nth)
            .map(|(i, _)| i)
        else {
            continue;
        };
        // With the rest of it, if it's hyphenated across lines.
        let parts = std::iter::once(words[start]).chain(
            words[start + 1..]
                .iter()
                .copied()
                .take_while(|w| w.continues),
        );
        for w in parts {
            let (x0, x1) = w.letters.unwrap_or((w.x0, w.x1));
            boxes.push(Rect {
                x0,
                y0: w.y0,
                x1,
                y1: w.y1,
            });
        }
    }
    boxes
}

/// Blocks set as a book page is, between the header and `bottom`.
fn set_pages(tools: &Tools, blocks: &[wordhord_book::model::Block], bottom: f32) -> Vec<Page> {
    let mut profile = tools.profile.clone();
    let mm = |px: f32| px * 25.4 / profile.ppi;
    profile.margins.top = mm(tools.ui.header_bottom());
    profile.margins.bottom = mm(profile.height as f32 - bottom);
    wordhord_typeset::reference_pages(blocks, "en", &profile, tools.fonts)
}

/// A word of the hoard on a page of its own: each sentence it was kept
/// from, the word in italic, with the book; then its dictionary entry.
fn word_page(tools: &Tools, k: usize, page: usize) -> Screen {
    use wordhord_book::model::BlockKind;
    let ui = tools.ui;
    let hoard = tools.hoard.borrow();
    let words = hoard.words();
    let Some(w) = words.get(k) else {
        return ui.list("The hoard", &[], 0, true);
    };
    let mut blocks: Vec<_> = w
        .kept
        .iter()
        .enumerate()
        .map(|(k, kept)| kept_quote(kept, &format!("kept-{k}")))
        .collect();
    if let Some(entry) = entry_of(tools, w.head, w.lang) {
        blocks.push(block(BlockKind::SceneBreak));
        blocks.extend(entry);
    }
    let set = set_pages(tools, &blocks, tools.profile.height as f32);
    let pages = set.len().max(1);
    let page = page.min(pages - 1);
    let content = set.into_iter().nth(page).unwrap_or_default();
    let marks = kept_marks(&content, &w.kept);
    let mut s = ui.titled(w.head, content, page, pages);
    s.marks = marks;
    s
}

/// A word of the hoard's dictionary entry: only its own readings, not
/// those of other words that list it among their forms.
fn entry_of(tools: &Tools, head: &str, lang: &str) -> Option<Vec<wordhord_book::model::Block>> {
    let (_, mut readings) = tools.lexicon.look_up(head, lang)?;
    if readings.iter().any(|r| r.entry.word == head) {
        readings.retain(|r| r.entry.word == head);
    }
    Some(lexicon::entry_blocks(head, &readings))
}

/// When a word graded so would come back, in words.
fn days(n: u64) -> String {
    match n {
        0 => "today".into(),
        1 => "tomorrow".into(),
        2..=13 => format!("{n} days"),
        14..=59 => format!("{} weeks", n / 7),
        60..=364 => format!("{} months", n / 30),
        365..=729 => "a year".into(),
        _ => format!("{} years", n / 365),
    }
}

/// Unlocking the hoard: each word due, as a card. First the sentence it
/// was kept from; a tap shows the entry, and a grade sends it on. A word
/// graded again comes round once more before the end. False if Wordhord
/// was closed.
fn unlock(screen: &mut dyn Platform, renderer: &mut Renderer, tools: &Tools) -> bool {
    use wordhord_book::model::BlockKind;
    let ui = tools.ui;
    let (width, height) = screen.size();
    let mut queue: std::collections::VecDeque<(String, String)> =
        tools.hoard.borrow().due(hoard::now()).into();
    let mut done = 0;
    while let Some((head, lang)) = queue.pop_front() {
        let (front, back, state, kept) = {
            let hoard = tools.hoard.borrow();
            let words = hoard.words();
            let Some(w) = words.iter().find(|w| w.head == head && w.lang == lang) else {
                continue;
            };
            // Another of its sentences each time, where there are several.
            let kept = w.kept[hoard.reviewed(&head, &lang) % w.kept.len()];
            let quote = kept_quote(kept, "kept-0");
            let mut back = vec![quote.clone(), block(BlockKind::SceneBreak)];
            back.extend(entry_of(tools, &head, &lang).unwrap_or_default());
            (vec![quote], back, hoard.state(&head, &lang), kept.clone())
        };
        let now = hoard::now();
        // Again comes round today; the rest, as FSRS has it.
        let when = [1, 2, 3, 4].map(|g| {
            if g == 1 {
                days(0)
            } else {
                days(fsrs::interval(&fsrs::next(state, g, now)))
            }
        });
        let mut shown = false;
        loop {
            let blocks = if shown { &back } else { &front };
            let content = set_pages(tools, blocks, ui.card_foot())
                .into_iter()
                .next()
                .unwrap_or_default();
            let marks = kept_marks(&content, &[&kept]);
            let s = ui.card(content, queue.len() + 1, shown.then(|| when.clone()));
            let pixels = highlight(
                &renderer.page(&s.page, width, height),
                width,
                height,
                &marks,
            );
            // A new card flashes the screen clean; the entry comes quietly.
            screen.draw(&pixels, width, height, 0, 0, !shown);
            let action = loop {
                match screen.next() {
                    Gesture::Power => return false,
                    Gesture::Tap(x, y) => {
                        if let Some(a) = s.hit(x as f32, y as f32) {
                            break a.clone();
                        }
                    }
                    _ => {}
                }
            };
            match action {
                Action::Back => return true,
                Action::Reveal => shown = true,
                Action::Grade(g) => {
                    if let Err(e) = tools.hoard.borrow_mut().review(&head, &lang, g) {
                        eprintln!("hoard: {e}");
                    }
                    eprintln!("unlock: {head:?} {}", fsrs::GRADES[g as usize - 1]);
                    if g == 1 {
                        queue.push_back((head.clone(), lang.clone()));
                    } else {
                        done += 1;
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    // The end: how many were unlocked, and how many wait for tomorrow.
    let tomorrow = tools.hoard.borrow().due(hoard::now() + fsrs::DAY).len();
    let words = |n: usize| if n == 1 { "word" } else { "words" };
    let mut lines = vec![if done > 0 {
        format!("{done} {} unlocked.", words(done))
    } else {
        "Nothing to unlock today.".to_string()
    }];
    if tomorrow > 0 {
        lines.push(format!("{tomorrow} {} tomorrow.", words(tomorrow)));
    }
    let s = ui.centred_note("unlocking", &lines);
    let pixels = renderer.page(&s.page, width, height);
    screen.draw(&pixels, width, height, 0, 0, true);
    !matches!(screen.next(), Gesture::Power)
}

/// The library, until a book is chosen (its path) or Wordhord is closed.
fn browse(
    screen: &mut dyn Platform,
    browser: &mut Browser,
    library: &Library,
    root: &Path,
    app: &Path,
    tools: &Tools,
) -> Option<String> {
    let fonts = tools.fonts;
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
        let s = browser.screen(tools, library, &recent);
        let started = Instant::now();
        let pixels = highlight(
            &renderer.page(&s.page, width, height),
            width,
            height,
            &s.marks,
        );
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
                    Some(Action::Unlock) => {
                        if !unlock(screen, &mut renderer, tools) {
                            return None;
                        }
                        flash = true;
                        break;
                    }
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
    hoard: &'a RefCell<Hoard>,
    light: &'a RefCell<Option<Light>>,
}

/// Where reading goes from a chapter.
enum Step {
    Next,
    Previous,
    Jump(usize),
    /// The type has changed: lay the book out again, at the page holding
    /// this word of the chapter.
    Retype(usize),
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
    let mut book = Book::open(&root.join(path), app)?;
    eprintln!(
        "{}: {} chapters, opened in {:?}",
        book.name,
        book.chapters(),
        started.elapsed()
    );
    let (mut chapter, mut page) = book.position();
    let mut flash = true;
    // Chapters laid out ahead, on another thread.
    let mut ahead = Ahead::start(&book);
    // After the type changes: the word to find in the new pages.
    let mut find: Option<usize> = None;
    // A renderer for the bar and the contents, in the app's own fonts.
    let mut chrome = Renderer::new(fonts);
    loop {
        // If this chapter is being laid out ahead, wait for it rather than
        // do the work twice.
        ahead.wait_for(chapter);
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
        if let Some(word) = find.take() {
            page = page_with(&t, word);
        }
        let mut reader = Reader::new(&t, page);
        let progress =
            |page: usize| book.progress(chapter, (page + 1) as f32 / t.pages.len() as f32);
        book.remember(chapter, reader.page);
        library::opened(app, path, progress(reader.page));
        // The page as drawn, to put back where the panel or bar was.
        let mut shown = draw(screen, &mut renderer, &t.pages[reader.page], flash);
        flash = false;
        ahead.reading(&book, chapter);
        // Where a light gauge is showing, to clear at the next touch.
        let mut gauge: Option<Rect> = None;
        let step = 'reading: loop {
            let gesture = screen.next();
            let effect = reader.act(gesture);
            eprintln!("{gesture:?}: {effect:?}");
            if let Some(r) = gauge.take() {
                // A new page, or another gauge, covers it anyway.
                if !matches!(effect, Effect::Page { .. } | Effect::Light { .. }) {
                    let (y0, y1) = (r.y0 as u32, r.y1 as u32);
                    let rows = &shown[(y0 * width) as usize..(y1 * width) as usize];
                    screen.draw(rows, width, y1 - y0, 0, y0, false);
                }
            }
            match effect {
                Effect::Light { warm, by } => {
                    let mut light = tools.light.borrow_mut();
                    let Some(light) = light.as_mut() else {
                        continue;
                    };
                    if warm && light.warmth().is_none() {
                        continue;
                    }
                    light.slide(warm, by as f32 / 1000.0);
                    let (name, figure, share) = match light.warmth().filter(|_| warm) {
                        Some(w) => ("warmth", w, w as f32 / light.max_warmth() as f32),
                        None => {
                            let b = light.brightness();
                            ("light", b, b as f32 / 100.0)
                        }
                    };
                    let (page, r) = ui.gauge(name, &figure.to_string(), share, !warm);
                    let (w, h) = ((r.x1 - r.x0) as u32, (r.y1 - r.y0) as u32);
                    let pixels = chrome.page(&page, w, h);
                    screen.draw(&pixels, w, h, r.x0 as u32, r.y0 as u32, false);
                    eprintln!("light: {name} {figure}");
                    gauge = Some(r);
                }
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
                    // Found: the pack it was found in, and the first
                    // reading's headword and part of speech, for the hoard.
                    let mut found = None;
                    let blocks = match tools.lexicon.look_up(&word, &lang) {
                        Some((pack, readings)) => {
                            found = readings.first().map(|r| {
                                (pack.to_string(), r.entry.word.clone(), r.entry.pos.clone())
                            });
                            lexicon::entry_blocks(&shown_as, &readings)
                        }
                        None => lexicon::not_found(&shown_as, &lang, !tools.lexicon.is_empty()),
                    };
                    let printed = word
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_string();
                    let word = shown_as;
                    // Which word of its paragraph or stanza this is, counted
                    // through the chapter as the pages split it.
                    let block = words[parts[0]].block.clone();
                    let nth = t.pages[..reader.page]
                        .iter()
                        .flat_map(|p| &p.words)
                        .chain(&words[..parts[0]])
                        .filter(|w| w.block == block && !w.continues)
                        .count();
                    let (mut area, pages) =
                        wordhord_typeset::panel_pages(&blocks, &lang, tools.profile, fonts);
                    // Under the panel, the word would be hidden: the panel
                    // comes down from the top instead.
                    let above = boxes.iter().any(|b| b.y1 > area.y);
                    if !above {
                        // Up to the top of any line it would cut through,
                        // so long as the word stays clear above it.
                        let cut = words
                            .iter()
                            .filter(|w| w.y0 < area.y && w.y1 > area.y)
                            .map(|w| w.y0)
                            .fold(area.y, f32::min);
                        if cut >= boxes.iter().map(|b| b.y1).fold(0.0, f32::max) {
                            area.height += area.y - cut.floor();
                            area.y = cut.floor();
                        }
                    }
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
                    let right = area.width - tools.profile.mm(tools.profile.margins.right);
                    let mut at = 0;
                    loop {
                        let mut pixels = render::panel(&mut chrome, &pages[at], w, h);
                        // Hoard it, or say it's hoarded: at the top right.
                        let kept = found.as_ref().and_then(|(pack, head, _)| {
                            let hoard = tools.hoard.borrow();
                            let k = hoard.find(head, pack, path, chapter, &block)?;
                            Some(k.id.clone())
                        });
                        let label = found.as_ref().map(|_| {
                            let text = if kept.is_some() {
                                "in the hoard"
                            } else {
                                "hoard"
                            };
                            let (page, hit) =
                                ui.label(text, kept.is_some(), right, tools.profile.mm(4.8));
                            let over = chrome.page(&page, w, h);
                            for (p, o) in pixels.iter_mut().zip(&over) {
                                *p = (*p).min(*o);
                            }
                            hit
                        });
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
                                    let (px, py) = (x as f32, y as f32 - area.y);
                                    if label.is_some_and(|r| r.contains(px, py)) {
                                        if let Some((pack, head, pos)) = &found {
                                            let mut hoard = tools.hoard.borrow_mut();
                                            let done = match &kept {
                                                Some(id) => hoard.let_go(id),
                                                None => {
                                                    let (sentence, at) = hoard::sentence(
                                                        &book.blocks(chapter),
                                                        &block,
                                                        nth,
                                                        &printed,
                                                    )
                                                    .unwrap_or_else(|| (printed.clone(), 0));
                                                    hoard.keep(Kept {
                                                        id: String::new(),
                                                        time: 0,
                                                        word: printed.clone(),
                                                        head: head.clone(),
                                                        lang: pack.clone(),
                                                        pos: pos.clone(),
                                                        sentence,
                                                        at,
                                                        book: book.title(),
                                                        path: path.to_string(),
                                                        chapter,
                                                        block: block.clone(),
                                                    })
                                                }
                                            };
                                            match done {
                                                Ok(()) => eprintln!(
                                                    "hoard: {head:?} {}",
                                                    if kept.is_some() { "let go" } else { "kept" }
                                                ),
                                                Err(e) => eprintln!("hoard: {e}"),
                                            }
                                        }
                                        break at;
                                    }
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
                        Some(Action::Type) => {
                            match type_settings(screen, &book, app, ui, &mut chrome, chapter) {
                                Some(Ok(())) => {
                                    // The first word on this page, counted
                                    // through the chapter.
                                    let word =
                                        t.pages[..reader.page].iter().map(|p| p.words.len()).sum();
                                    break Step::Retype(word);
                                }
                                Some(Err(leave)) => break Step::Leave(leave),
                                None => {
                                    shown = draw(screen, &mut renderer, &t.pages[reader.page], true)
                                }
                            }
                        }
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
            Step::Retype(word) => {
                book = Book::open(&root.join(path), app)?;
                ahead = Ahead::start(&book);
                find = Some(word);
                flash = true;
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

/// Lays out chapters ahead of the reader, on a thread of its own at a
/// lower priority, with its own copy of the book, opened once. Told the
/// chapter being read, it lays out the next chapter, then readies the
/// images of both, so they're waiting in the cache when the reader gets
/// there.
struct Ahead {
    chapters: Option<Sender<usize>>,
    /// The chapter being laid out now, if any.
    busy: Arc<(Mutex<Option<usize>>, Condvar)>,
}

impl Ahead {
    fn start(book: &Book) -> Ahead {
        let (path, app) = (book.path.clone(), book.app.clone());
        let (tx, rx) = channel::<usize>();
        let busy = Arc::new((Mutex::new(None), Condvar::new()));
        let shared = Arc::clone(&busy);
        std::thread::spawn(move || {
            // On Linux, nice applies to the calling thread alone.
            unsafe {
                libc::nice(10);
            }
            let mut book: Option<Book> = None;
            while let Ok(mut now) = rx.recv() {
                // Only the latest chapter matters.
                while let Ok(later) = rx.try_recv() {
                    now = later;
                }
                if book.is_none() {
                    book = Book::open(&path, &app).ok();
                }
                if let Some(b) = &book {
                    let started = Instant::now();
                    let next = now + 1;
                    if next < b.chapters() && !b.is_cached(next) {
                        match b.chapter(next) {
                            Ok(t) => {
                                eprintln!(
                                    "ahead: chapter {next} laid out in {:?}",
                                    started.elapsed()
                                );
                                // Done with the layout: the reader needn't
                                // wait for the images.
                                free(&shared);
                                b.prepare_pictures(&t);
                            }
                            Err(e) => eprintln!("ahead: chapter {next}: {e}"),
                        }
                    }
                    free(&shared);
                    if let Ok(t) = b.chapter(now) {
                        b.prepare_pictures(&t);
                    }
                }
                free(&shared);
            }
        });
        Ahead {
            chapters: Some(tx),
            busy,
        }
    }

    /// The reader is on chapter `k` of `book`. The next chapter is marked
    /// as being laid out before the thread hears of it, so a quick turn to
    /// it waits rather than lays it out a second time.
    fn reading(&self, book: &Book, k: usize) {
        let Some(tx) = &self.chapters else { return };
        let next = k + 1;
        if next < book.chapters() && !book.is_cached(next) {
            *self.busy.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(next);
        }
        let _ = tx.send(k);
    }

    /// Waits while chapter `k` is being laid out.
    fn wait_for(&self, k: usize) {
        let (lock, done) = &*self.busy;
        let mut busy = lock.lock().unwrap_or_else(|e| e.into_inner());
        if *busy != Some(k) {
            return;
        }
        let waited = Instant::now();
        // Not forever, should the thread have stopped.
        let limit = std::time::Duration::from_secs(10);
        while *busy == Some(k) && waited.elapsed() < limit {
            busy = done
                .wait_timeout(busy, limit - waited.elapsed())
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        eprintln!("waited {:?} for chapter {k}", waited.elapsed());
    }
}

/// Marks nothing as being laid out, and wakes the reader if it's waiting.
fn free(busy: &(Mutex<Option<usize>>, Condvar)) {
    *busy.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    busy.1.notify_all();
}

impl Drop for Ahead {
    /// The thread ends once it's done with what it's doing.
    fn drop(&mut self) {
        self.chapters.take();
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

/// The page of `t` that holds its `word`th word.
fn page_with(t: &Typeset, word: usize) -> usize {
    let mut seen = 0;
    for (k, p) in t.pages.iter().enumerate() {
        seen += p.words.len();
        if word < seen {
            return k;
        }
    }
    t.pages.len().saturating_sub(1)
}

/// The specimen on the type screen: the same passage every time, with
/// italics, figures, and the letters of Old English and Old Norse, so
/// faces can be told apart. Italic between asterisks.
const SPECIMEN: &str = "When Beowulf answers the coastguard on the Danish shore, the poet \
says he *wordhord onleac*: he unlocked his word-hoard. That is line 259, written down \
around the year 1000. The poem opens *Hwæt! wē Gār-Dena in ġēar-dagum*, listen, we have \
heard of the Spear-Danes in days of old. The Norse had their own figure for it. Poetry \
was Óðinn’s mead, stolen from a giant and carried home in his belly. The *Vǫluspá* \
begins with a call for a hearing: *Hljóðs bið ek allar helgar kindir*, hear me, all \
you holy kin. Snorri Sturluson told the story of the mead in his *Edda* in the 1220s, \
for poets who no longer knew the old kennings.";

/// The specimen as a paragraph.
fn specimen() -> wordhord_book::model::Block {
    use wordhord_book::model::{Block, BlockKind, Inline};
    let inlines = SPECIMEN
        .split('*')
        .enumerate()
        .filter(|(_, t)| !t.is_empty())
        .map(|(k, t)| {
            let text = Inline::Text(t.to_string());
            if k % 2 == 1 {
                Inline::Emphasis(vec![text])
            } else {
                text
            }
        })
        .collect();
    Block {
        id: Some("specimen".into()),
        kind: BlockKind::Paragraph(inlines),
        lang: None,
        reference: None,
        line: 0,
    }
}

/// The type screen. Each choice sets the specimen again, in the type
/// chosen. Changed settings are saved, and `Ok`; unchanged, `None`.
fn type_settings(
    screen: &mut dyn Platform,
    book: &Book,
    app: &Path,
    ui: &Ui,
    chrome: &mut Renderer,
    chapter: usize,
) -> Option<Result<(), Leave>> {
    use wordhord_book::model;
    let (width, height) = screen.size();
    let base = Profile::load(&app.join("profile.toml")).ok()?;
    let mut chosen = Settings::load(app);
    let before = chosen.clone();
    let specimen = specimen();
    let blocks = book.blocks(chapter);
    // Every letter the chapter uses, to say which a face lacks.
    let mut all = Vec::new();
    model::walk(&blocks, &mut all);
    let mut letters: Vec<char> = all
        .iter()
        .flat_map(|b| model::block_inlines(b))
        .flat_map(|i| model::plain_text(i).chars().collect::<Vec<_>>())
        .filter(|c| c.is_alphabetic())
        .collect();
    letters.sort_unstable();
    letters.dedup();
    // Each face's regular, for its name and the languages it can set.
    let faces: Vec<Option<Fonts>> = settings::FACES
        .iter()
        .map(|f| {
            let file = format!("fonts/{}", f.files[0]);
            let paths = FontPaths {
                regular: file.clone(),
                italic: file.clone(),
                bold: file.clone(),
                bold_italic: file,
                mono: None,
                fallback: None,
            };
            Fonts::load(&base.dir, &paths).ok()
        })
        .collect();
    let languages: Vec<Vec<&str>> = faces
        .iter()
        .map(|f| f.as_ref().map(settings::languages).unwrap_or_default())
        .collect();
    let rows: [(Choice, &str, Vec<&str>); 4] = [
        (
            Choice::Size,
            "size",
            settings::SIZES.iter().map(|s| s.0).collect(),
        ),
        (
            Choice::Measure,
            "measure",
            settings::MEASURES.iter().map(|s| s.0).collect(),
        ),
        (
            Choice::Leading,
            "leading",
            settings::LEADINGS.iter().map(|s| s.0).collect(),
        ),
        (
            Choice::Setting,
            "setting",
            settings::SETTINGS.iter().map(|s| s.0).collect(),
        ),
    ];
    let mut flash = true;
    loop {
        let started = Instant::now();
        let at = |names: &[&str], name: &str| names.iter().position(|n| *n == name).unwrap_or(0);
        let picked = [
            settings::FACES
                .iter()
                .position(|f| f.key == chosen.typeface().key)
                .unwrap_or(0),
            at(&rows[0].2, &chosen.size),
            at(&rows[1].2, &chosen.measure),
            at(&rows[2].2, &chosen.leading),
            at(&rows[3].2, &chosen.setting),
        ];
        let (profile, fonts) = chosen.apply(&base).ok()?;
        let lacking: String = letters
            .iter()
            .filter(|c| !fonts.covers(**c))
            .take(8)
            .collect();
        let note = (!lacking.is_empty()).then(|| {
            format!(
                "{} lacks {}: set in DejaVu Serif.",
                chosen.typeface().name,
                lacking
                    .chars()
                    .map(String::from)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        });
        let ts = ui.type_screen(&rows, &picked, &languages, note.as_deref());
        let mut pixels = chrome.page(&ts.screen.page, width, height);
        // The specimen, from the top of a page set as chosen.
        let set = wordhord_typeset::lay_out(
            std::slice::from_ref(&specimen),
            // The specimen is in English, whatever the book.
            "en",
            &profile,
            &fonts,
            &[],
            None,
        );
        if let Some(first) = set.pages.first() {
            let page = Renderer::new(&fonts).page(first, width, height);
            let from = (profile.mm(profile.margins.top) - profile.mm(3.0)).max(0.0) as usize;
            let (y0, y1) = (ts.specimen.y0 as usize, ts.specimen.y1 as usize);
            let w = width as usize;
            // Whole lines only: none cut by the rule under it.
            let pitch = profile.pt(profile.body_size) * profile.leading;
            let lines = ((y1 - y0) as f32 - profile.mm(3.0)) / pitch;
            let whole = (profile.mm(3.0) + lines.floor().max(1.0) * pitch) as usize;
            let rows = whole.min(y1 - y0).min(height as usize - from);
            pixels[y0 * w..(y0 + rows) * w].copy_from_slice(&page[from * w..(from + rows) * w]);
        }
        // Each face's name, in that face.
        for (k, (x, baseline, size)) in ts.names.iter().enumerate() {
            let Some(f) = faces.get(k).and_then(|f| f.as_ref()) else {
                continue;
            };
            let band = (size * 2.0) as u32;
            let top = (baseline - size * 1.4).max(0.0) as u32;
            let mut p = Page::default();
            let mut cx = *x;
            for s in f.shape(
                settings::FACES[k].name,
                wordhord_typeset::fonts::Face::Regular,
                *size,
                &["onum"],
            ) {
                p.runs.push(Run {
                    font: s.font,
                    size: s.size,
                    x: cx,
                    y: size * 1.4,
                    glyphs: s.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
                });
                cx += s.width;
            }
            let name = Renderer::new(f).page(&p, width, band);
            for (row, line) in name.chunks(width as usize).enumerate() {
                let y = top as usize + row;
                if y >= height as usize {
                    break;
                }
                let dst = &mut pixels[y * width as usize..(y + 1) * width as usize];
                for (d, s) in dst.iter_mut().zip(line) {
                    *d = (*d).min(*s);
                }
            }
        }
        screen.draw(&pixels, width, height, 0, 0, flash);
        flash = false;
        eprintln!("type: {} drawn in {:?}", chosen.key(), started.elapsed());
        let action = loop {
            match screen.next() {
                Gesture::Tap(x, y) => {
                    if let Some(a) = ts.screen.hit(x as f32, y as f32) {
                        break a.clone();
                    }
                }
                Gesture::Power => return Some(Err(Leave::Close)),
                _ => {}
            }
        };
        let name = |list: &[&str], k: usize| list.get(k).map(|n| n.to_string());
        match action {
            Action::Back => break,
            Action::Choose(Choice::Face, k) => {
                chosen.face = settings::FACES.get(k).map_or(chosen.face, |f| f.key.into())
            }
            Action::Choose(Choice::Size, k) => {
                chosen.size = name(&rows[0].2, k).unwrap_or(chosen.size)
            }
            Action::Choose(Choice::Measure, k) => {
                chosen.measure = name(&rows[1].2, k).unwrap_or(chosen.measure)
            }
            Action::Choose(Choice::Leading, k) => {
                chosen.leading = name(&rows[2].2, k).unwrap_or(chosen.leading)
            }
            Action::Choose(Choice::Setting, k) => {
                chosen.setting = name(&rows[3].2, k).unwrap_or(chosen.setting)
            }
            _ => {}
        }
    }
    if chosen == before {
        return None;
    }
    if let Err(e) = chosen.save(app) {
        eprintln!("type: {e}");
        return None;
    }
    Some(Ok(()))
}
