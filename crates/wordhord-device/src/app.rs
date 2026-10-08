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
use crate::marks::{Anchor, Ink, Marks, Underline, Words};
use crate::platform::Platform;
use crate::reader::{Effect, Reader};
use crate::settings::{self, Settings};
use crate::touch::{Gesture, Tool};
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
    let marks = RefCell::new(Marks::load(app));
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
        .find(|p| library.books.iter().any(|b| &b.path == p))
        .map(|p| (p, None));
    let mut browser = Browser::default();
    let tools = Tools {
        ui: &ui,
        fonts: &fonts,
        profile: &profile,
        lexicon: &lexicon,
        hoard: &hoard,
        light: &light,
        marks: &marks,
    };
    loop {
        if let Some((path, at)) = open.take() {
            match read(screen, root, &path, at, app, &tools) {
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
            Some(book) => open = Some(book),
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
    Commonplace,
    /// One book's passages, by its place in the commonplace book's index.
    Passages(usize),
    /// What a search found, of the library's books or the commonplace
    /// book's passages: the words asked for are the browser's.
    FoundBooks,
    FoundPassages,
}

/// Where in the library the reader is: the views gone through, each with
/// its page.
#[derive(Default)]
struct Browser {
    stack: Vec<(View, usize)>,
    /// What was last searched for, to start from again.
    query: String,
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
            Action::Commonplace => self.stack.push((View::Commonplace, 0)),
            Action::Passages(k) => self.stack.push((View::Passages(*k), 0)),
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
                    tools.marks.borrow().count(),
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
            View::Commonplace => {
                let books = tools.marks.borrow().books();
                // First a way to search it, as a book's contents have.
                let search = Row {
                    title: "Search the commonplace book".into(),
                    detail: Vec::new(),
                    cover: None,
                    progress: None,
                    count: None,
                    mark: false,
                    action: Action::SearchNotes,
                    depth: 0,
                };
                let rows: Vec<Row> = std::iter::once(search)
                    .chain(books.iter().enumerate().map(|(k, (path, title, n))| {
                        Row {
                            title: title.clone(),
                            // As the shelves give a book: its author in small
                            // caps under its title.
                            detail: library
                                .books
                                .iter()
                                .find(|b| &b.path == path)
                                .map(|e| vec![Part::Caps(e.byline().0)])
                                .unwrap_or_default(),
                            cover: None,
                            progress: None,
                            count: Some(*n),
                            mark: false,
                            action: Action::Passages(k),
                            depth: 0,
                        }
                    }))
                    .collect();
                ui.list("Commonplace book", &rows, page, true)
            }
            View::Passages(k) => {
                let books = tools.marks.borrow().books();
                let Some((path, title, _)) = books.get(k) else {
                    return ui.list("Commonplace book", &[], 0, true);
                };
                passages(tools, |u| &u.path == path, title, false, page)
            }
            View::FoundPassages => {
                let q = self.query.as_str();
                let found = |u: &Underline| {
                    let hay = format!(
                        "{} {} {}",
                        u.text,
                        u.book,
                        u.section.as_deref().unwrap_or("")
                    );
                    crate::search::matches(&hay, q)
                };
                let any = tools.marks.borrow().by_book().into_iter().any(found);
                let heading = if any {
                    format!("“{q}”")
                } else {
                    format!("Nothing for “{q}”")
                };
                passages(tools, found, &heading, true, page)
            }
            View::FoundBooks => {
                let q = self.query.as_str();
                let rows: Vec<Row> = library
                    .books
                    .iter()
                    .filter(|e| {
                        // The language by name too: "norse" finds the
                        // Edda.
                        let language = e
                            .language
                            .as_deref()
                            .map(|l| lexicon::language_name(&lexicon::three_letter(l)).to_string())
                            .unwrap_or_default();
                        let hay = format!(
                            "{} {} {} {} {language}",
                            e.title,
                            e.authors.join(" "),
                            e.series.as_deref().unwrap_or(""),
                            e.tags.join(" ")
                        );
                        crate::search::matches(&hay, q)
                    })
                    .map(row)
                    .collect();
                let title = if rows.is_empty() {
                    format!("Nothing for “{q}”")
                } else {
                    format!("“{q}”")
                };
                ui.list(&title, &rows, page, true)
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
    let mut kept: Vec<&str> = w.kept.iter().map(|k| k.pos.as_str()).collect();
    kept.sort_unstable();
    kept.dedup();
    if let Some(entry) = entry_of(tools, w.head, w.lang, &kept) {
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

/// A word of the hoard's dictionary entry: only the readings kept, as
/// parts of speech, not the word's others or those of words that list it
/// among their forms.
fn entry_of(
    tools: &Tools,
    head: &str,
    lang: &str,
    kept: &[&str],
) -> Option<Vec<wordhord_book::model::Block>> {
    let (_, mut readings) = tools.lexicon.look_up(head, lang)?;
    let chosen =
        |r: &wordhord_dict::Reading| r.entry.word == head && kept.contains(&r.entry.pos.as_str());
    if readings.iter().any(chosen) {
        readings.retain(chosen);
    } else if readings.iter().any(|r| r.entry.word == head) {
        readings.retain(|r| r.entry.word == head);
    }
    Some(lexicon::entry_blocks(head, &readings))
}

/// An underline of the words `a` to `b` of a page, either way round: its
/// anchors, its text (lines of verse marked " / "), and the part of the
/// book it's in.
#[allow(clippy::too_many_arguments)]
fn underline_of(
    book: &Book,
    path: &str,
    chapter: usize,
    page: &Page,
    numbers: &[usize],
    numbered: &Words,
    a: usize,
    b: usize,
) -> Underline {
    let (a, b) = (a.min(b), a.max(b));
    let first = &page.words[a];
    let verse = hoard::is_verse(&book.blocks(chapter), &first.block);
    let mut text = String::new();
    let mut line = first.y0;
    for w in page.words.iter().skip(a).take(b - a + 1) {
        if !text.is_empty() && !w.continues {
            let turned = (w.y0 - line).abs() > 1.0;
            text.push_str(if verse && turned { " / " } else { " " });
        }
        line = w.y0;
        text.push_str(&w.text);
    }
    // The part of the book it's in: the last entry of the contents at or
    // before this chapter.
    let section = book
        .contents()
        .into_iter()
        .rfind(|c| c.2 <= chapter)
        .map(|c| c.0);
    Underline {
        id: String::new(),
        time: 0,
        book: book.title(),
        path: path.to_string(),
        chapter,
        start: numbered.anchors[numbers[a]].clone(),
        end: numbered.anchors[numbers[b]].clone(),
        text,
        section,
    }
}

/// What's drawn over a page of a book: underlines, as rules; and strokes
/// of the pen, each a run of points with the pen's radius there.
#[derive(Default)]
struct Decor {
    rules: Vec<Rect>,
    strokes: Vec<Stroke>,
}

/// A stroke of the pen on the screen: points, each with the pen's radius.
type Stroke = Vec<(f32, f32, f32)>;

/// The pen's radius in pixels at a pressure, 0 to 4095: a fine line,
/// fuller as it's pressed.
fn pen_radius(pressure: u16) -> f32 {
    1.2 + 1.8 * (pressure as f32 / 4095.0).powf(0.8)
}

/// A stroke inked into a screen's pixels: discs along it, close enough to
/// join. Where it went, if anywhere.
fn ink_into(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stroke: &[(f32, f32, f32)],
) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    let mut disc = |x: f32, y: f32, r: f32| {
        let (x0, x1) = (
            (x - r).floor().max(0.0) as u32,
            ((x + r).ceil() as u32).min(width),
        );
        let (y0, y1) = (
            (y - r).floor().max(0.0) as u32,
            ((y + r).ceil() as u32).min(height),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                if dx * dx + dy * dy <= r * r {
                    pixels[(py * width + px) as usize] = 0;
                }
            }
        }
        let b = bounds.get_or_insert(Rect {
            x0: x0 as f32,
            y0: y0 as f32,
            x1: x1 as f32,
            y1: y1 as f32,
        });
        b.x0 = b.x0.min(x0 as f32);
        b.y0 = b.y0.min(y0 as f32);
        b.x1 = b.x1.max(x1 as f32);
        b.y1 = b.y1.max(y1 as f32);
    };
    if let [(x, y, r)] = stroke {
        disc(*x, *y, *r);
    }
    for pair in stroke.windows(2) {
        let ((ax, ay, ar), (bx, by, br)) = (pair[0], pair[1]);
        let steps = ((bx - ax).hypot(by - ay) / 0.7).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let f = k as f32 / steps as f32;
            disc(ax + (bx - ax) * f, ay + (by - ay) * f, ar + (br - ar) * f);
        }
    }
    bounds.filter(|b| b.x1 > b.x0 && b.y1 > b.y0)
}

/// A page's margins, as where each starts across and how wide it is: the
/// left, from the page's edge to the text; the right, from the text to the
/// page's edge. The text is as wide as its words reach.
fn text_edges(page: &Page, width: u32) -> ((f32, f32), (f32, f32)) {
    let x0 = page.words.iter().map(|w| w.x0).fold(f32::MAX, f32::min);
    let x1 = page.words.iter().map(|w| w.x1).fold(0.0, f32::max);
    let (x0, x1) = if x0 < x1 {
        (x0, x1)
    } else {
        (0.0, width as f32)
    };
    ((0.0, x0.max(1.0)), (x1, (width as f32 - x1).max(1.0)))
}

/// The strokes of a page, placed from the words they're anchored to.
fn strokes(
    page: &Page,
    numbered: &Words,
    k: usize,
    marks: &Marks,
    path: &str,
    chapter: usize,
    page_width: u32,
) -> Vec<(String, Stroke)> {
    let numbers = &numbered.on_page[k];
    marks
        .ink_in(path, chapter)
        .filter_map(|ink| {
            let n = numbered.anchors.iter().position(|a| *a == ink.anchor)?;
            let w = &page.words[numbers.iter().position(|&m| m == n)?];
            let lh = w.y1 - w.y0;
            let (left, right) = text_edges(page, page_width);
            let start = ink.points.first().map_or(0.0, |p| p[0]);
            let first = match ink.margin.as_deref() {
                Some("right") => Some(right.0 + start * right.1),
                Some("left") => Some(left.0 + start * left.1),
                _ => None,
            };
            let mut points: Vec<(f32, f32, f32)> = ink
                .points
                .iter()
                .enumerate()
                .map(|(k, [u, v, r])| {
                    let x = match first {
                        Some(f) if k == 0 => f,
                        Some(f) => f + u * lh,
                        None => w.x0 + u * lh,
                    };
                    (x, w.y0 + v * lh, r * lh)
                })
                .collect();
            // A margin's stroke kept on the page.
            if first.is_some() {
                let lo = points.iter().map(|p| p.0 - p.2).fold(f32::MAX, f32::min);
                let hi = points.iter().map(|p| p.0 + p.2).fold(0.0, f32::max);
                let shift = if hi > page_width as f32 {
                    page_width as f32 - hi
                } else if lo < 0.0 {
                    -lo
                } else {
                    0.0
                };
                for p in &mut points {
                    p.0 += shift;
                }
            }
            Some((ink.id.clone(), points))
        })
        .collect()
}

/// Underlines, drawn in: dark rules in the paper.
fn rule(pixels: &mut [u8], width: u32, height: u32, rules: &[Rect]) {
    for r in rules {
        let (x0, x1) = (r.x0.max(0.0) as u32, (r.x1 as u32).min(width));
        for y in r.y0.max(0.0) as u32..(r.y1 as u32).min(height) {
            let row = (y * width) as usize;
            for p in &mut pixels[row + x0 as usize..row + x1 as usize] {
                *p = (*p).min(60);
            }
        }
    }
}

/// The pixels of a screen inside `r`, as a block of their own.
fn cut(pixels: &[u8], width: u32, r: &Rect) -> Vec<u8> {
    let (x0, x1) = (r.x0.max(0.0) as usize, (r.x1 as usize).min(width as usize));
    let mut out = Vec::with_capacity((x1 - x0) * (r.y1 - r.y0) as usize);
    for y in r.y0.max(0.0) as usize..r.y1 as usize {
        let row = y * width as usize;
        out.extend_from_slice(&pixels[row + x0..row + x1]);
    }
    out
}

/// A block of grey pixels laid over a screen's at `r`.
fn paste(pixels: &mut [u8], width: u32, height: u32, block: &[u8], r: &Rect) {
    let (x0, y0) = (r.x0.max(0.0) as u32, r.y0.max(0.0) as u32);
    let bw = (r.x1 - r.x0) as u32;
    let (w, h) = (bw.min(width - x0), ((r.y1 - r.y0) as u32).min(height - y0));
    for row in 0..h {
        let dst = ((y0 + row) * width + x0) as usize;
        let src = (row * bw) as usize;
        pixels[dst..dst + w as usize].copy_from_slice(&block[src..src + w as usize]);
    }
}

/// Runs of words on a page that `chosen` picks, each within one line: the
/// first and last of each, by their place on the page.
fn runs(page: &Page, chosen: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (k, w) in page.words.iter().enumerate() {
        if !chosen(k) {
            continue;
        }
        match out.last_mut() {
            Some((_, last)) if *last + 1 == k && (page.words[*last].y0 - w.y0).abs() < 1.0 => {
                *last = k
            }
            _ => out.push((k, k)),
        }
    }
    out
}

/// A run of words as one box, punctuation and all: a passage is kept with
/// its stops, so it's drawn with them.
fn run_box(page: &Page, (a, b): (usize, usize)) -> Rect {
    let (first, last) = (&page.words[a], &page.words[b]);
    Rect {
        x0: first.x0,
        y0: first.y0,
        x1: last.x1,
        y1: first.y1,
    }
}

/// The underlines on a page, as rules under their words.
fn underlines(
    page: &Page,
    numbers: &[usize],
    spans: &[(usize, usize)],
    fonts: &Fonts,
) -> Vec<Rect> {
    let (ascent, descent) = fonts.ascent_descent(1.0);
    runs(page, |k| {
        let n = numbers[k];
        spans.iter().any(|&(s, e)| (s..=e).contains(&n))
    })
    .into_iter()
    .map(|run| {
        let b = run_box(page, run);
        let baseline = b.y0 + (b.y1 - b.y0) * ascent / (ascent + descent);
        // Under the letters, clear of most descenders' tails.
        let y = (baseline + (b.y1 - baseline) * 0.45).round();
        let thick = ((b.y1 - b.y0) * 0.035).round().max(2.0);
        Rect {
            y0: y,
            y1: y + thick,
            ..b
        }
    })
    .collect()
}

/// The word nearest a point on a page: on the line nearest it, the word
/// nearest along it. For a finger dragged between lines or past the end.
fn nearest_word(page: &Page, x: f32, y: f32) -> Option<usize> {
    let line = |w: &wordhord_typeset::page::Word| (w.y0 - y).max(y - w.y1).max(0.0);
    let along = |w: &wordhord_typeset::page::Word| (w.x0 - x).max(x - w.x1).max(0.0);
    page.words
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            line(a)
                .total_cmp(&line(b))
                .then(along(a).total_cmp(&along(b)))
        })
        .map(|(k, _)| k)
}

/// Passages of the commonplace book that `select` picks, under `heading`:
/// set as quotations, each with the part of the book it's in and the day
/// it was underlined; and with the book, if `with_book`, where they may
/// come from several. A tap on one opens the book there.
fn passages(
    tools: &Tools,
    select: impl Fn(&Underline) -> bool,
    heading: &str,
    with_book: bool,
    page: usize,
) -> Screen {
    use wordhord_book::model::{Block, BlockKind, Inline};
    let ui = tools.ui;
    let marks = tools.marks.borrow();
    let all = marks.by_book();
    let mut blocks = Vec::new();
    for (n, u) in all.iter().enumerate().filter(|(_, u)| select(u)) {
        let id = format!("passage-{n}");
        let with_id = |b: Block| Block {
            id: Some(id.clone()),
            ..b
        };
        let day = crate::marks::day(u.time);
        let mut whence: Vec<&str> = Vec::new();
        if with_book {
            whence.push(&u.book);
        }
        if let Some(s) = &u.section {
            whence.push(s);
        }
        whence.push(&day);
        let whence = whence.join(" · ");
        blocks.push(with_id(block(BlockKind::Quote(vec![
            with_id(block(BlockKind::Paragraph(vec![Inline::Text(
                u.text.clone(),
            )]))),
            with_id(block(BlockKind::Paragraph(vec![Inline::Emphasis(vec![
                Inline::Text(whence),
            ])]))),
        ]))));
    }
    let set = set_pages(tools, &blocks, tools.profile.height as f32);
    let pages = set.len().max(1);
    let page = page.min(pages - 1);
    let content = set.into_iter().nth(page).unwrap_or_default();
    // Each passage, wherever its words are on the page, is a place to tap.
    let mut hits: Vec<(Rect, Action)> = Vec::new();
    for w in &content.words {
        let Some(n) = w
            .block
            .strip_prefix("passage-")
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        match hits.iter_mut().find(|(_, a)| *a == Action::Passage(n)) {
            Some((r, _)) => {
                r.y0 = r.y0.min(w.y0);
                r.y1 = r.y1.max(w.y1);
            }
            None => hits.push((
                Rect {
                    x0: 0.0,
                    y0: w.y0,
                    x1: tools.profile.width as f32,
                    y1: w.y1,
                },
                Action::Passage(n),
            )),
        }
    }
    let mut s = ui.titled(heading, content, page, pages);
    s.hits.extend(hits);
    s
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
            back.extend(entry_of(tools, &head, &lang, &[&kept.pos]).unwrap_or_default());
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
) -> Option<(String, Place)> {
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
                    Some(Action::Open(path)) => return Some((path.clone(), None)),
                    Some(a @ (Action::SearchLibrary | Action::SearchNotes)) => {
                        let (title, view) = if *a == Action::SearchLibrary {
                            ("Search the library", View::FoundBooks)
                        } else {
                            ("Search the commonplace book", View::FoundPassages)
                        };
                        match type_in(screen, tools.ui, &mut renderer, title, &mut browser.query) {
                            Typed::Done => browser.stack.push((view, 0)),
                            Typed::Back => {}
                            Typed::Close => return None,
                        }
                        flash = true;
                        break;
                    }
                    Some(Action::Passage(k)) => {
                        let marks = tools.marks.borrow();
                        if let Some(u) = marks.by_book().get(*k) {
                            return Some((u.path.clone(), Some((u.chapter, u.start.clone()))));
                        }
                    }
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
    marks: &'a RefCell<Marks>,
}

/// Where reading goes from a chapter.
enum Step {
    Next,
    Previous,
    /// To a chapter, at a word of it if given.
    Jump(usize, Arrival),
    /// The type has changed: lay the book out again, at the page holding
    /// this word of the chapter.
    Retype(usize),
    Leave(Leave),
}

/// A word to arrive at in a chapter, and the words found there, to mark
/// grey: none for a passage, which has its underline.
type Arrival = Option<(Anchor, Vec<String>)>;

/// A place in a book to open it at: a chapter, and a word of it.
type Place = Option<(usize, Anchor)>;

fn read(
    screen: &mut dyn Platform,
    root: &Path,
    path: &str,
    at: Place,
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
    // Opened at a passage: its chapter now, its page once laid out.
    // With it, how many of its words to mark grey on arrival: the words
    // searched for.
    let mut find_anchor: Arrival = None;
    if let Some((c, anchor)) = at.filter(|a| a.0 < book.chapters()) {
        chapter = c;
        find_anchor = Some((anchor, Vec::new()));
    }
    let mut arrive_mark: Option<(usize, usize)> = None;
    let mut flash = true;
    // Chapters laid out ahead, on another thread.
    let mut ahead = Ahead::start(&book);
    // After the type changes: the word to find in the new pages.
    let mut find: Option<usize> = None;
    // A renderer for the bar and the contents, in the app's own fonts.
    let mut chrome = Renderer::new(fonts);
    // The book's text for searching, kept while it's open.
    let search_text = crate::search::Text::default();
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
        if let Some((anchor, marked)) = find_anchor.take() {
            // The word itself; or, if the pages count its paragraph's words
            // a little differently, the nearest before it there.
            let words = Words::of(&t);
            let n = words.anchors.iter().position(|a| *a == anchor).or_else(|| {
                words
                    .anchors
                    .iter()
                    .rposition(|a| a.block == anchor.block && a.word <= anchor.word)
            });
            // Words searched for: the search and the pages may count a
            // paragraph's words a little differently, so the nearest word
            // that reads as the first found.
            let n = match (n, marked.first()) {
                (Some(n), Some(first)) => {
                    let want = crate::search::loose(first);
                    let text_of = |m: usize| {
                        words.on_page.iter().zip(&t.pages).find_map(|(nums, p)| {
                            nums.iter()
                                .position(|&x| x == m)
                                .map(|i| p.words[i].text.clone())
                        })
                    };
                    (n.saturating_sub(6)..n + 7)
                        .filter(|&m| text_of(m).is_some_and(|w| crate::search::loose(&w) == want))
                        .min_by_key(|&m| m.abs_diff(n))
                        .or(Some(n))
                }
                (n, _) => n,
            };
            page = n
                .and_then(|n| words.on_page.iter().position(|p| p.contains(&n)))
                .unwrap_or(0);
            arrive_mark = n.filter(|_| !marked.is_empty()).map(|n| (n, marked.len()));
        }
        let mut reader = Reader::new(&t, page);
        let progress =
            |page: usize| book.progress(chapter, (page + 1) as f32 / t.pages.len() as f32);
        book.remember(chapter, reader.page);
        library::opened(app, path, progress(reader.page));
        // The page as drawn, to put back where the panel or bar was.
        // Each word of the chapter numbered, and the underlines' spans in
        // those numbers.
        let numbered = Words::of(&t);
        let spans = || -> Vec<(usize, usize)> {
            tools
                .marks
                .borrow()
                .in_chapter(path, chapter)
                .filter_map(|u| numbered.span(u))
                .collect()
        };
        // What's drawn over a page: its underlines and its strokes of ink.
        let rules = |page: usize| Decor {
            rules: underlines(&t.pages[page], &numbered.on_page[page], &spans(), fonts),
            strokes: strokes(
                &t.pages[page],
                &numbered,
                page,
                &tools.marks.borrow(),
                path,
                chapter,
                width,
            )
            .into_iter()
            .map(|(_, points)| points)
            .collect(),
        };
        let mut shown = draw(
            screen,
            &mut renderer,
            &t.pages[reader.page],
            flash,
            &rules(reader.page),
        );
        // Arrived at words searched for: grey, as a word looked up is,
        // until the page is next drawn.
        if let Some((n, count)) = arrive_mark.take() {
            let numbers = &numbered.on_page[reader.page];
            let page_now = &t.pages[reader.page];
            let boxes: Vec<Rect> = runs(page_now, |k| (n..n + count).contains(&numbers[k]))
                .into_iter()
                .map(|r| run_box(page_now, r))
                .collect();
            let marked = highlight(&shown, width, height, &boxes);
            let top = boxes
                .iter()
                .map(|b| b.y0.max(0.0) as u32)
                .min()
                .unwrap_or(0);
            let bottom = boxes
                .iter()
                .map(|b| b.y1 as u32)
                .max()
                .unwrap_or(0)
                .min(height);
            if bottom > top {
                let rows = &marked[(top * width) as usize..(bottom * width) as usize];
                screen.draw(rows, width, bottom - top, 0, top, false);
            }
        }
        // A passage being chosen to underline: its first and last words,
        // by their place on the page; and the rows last drawn marked.
        let mut chosen: Option<(usize, usize)> = None;
        let mut marked_rows: Option<(u32, u32)> = None;
        // Pressed on an underline already there: its id, and the word.
        let mut editing: Option<(String, usize)> = None;
        // The stroke the pen is drawing: its points, with the pen's radius.
        let mut stroke: Vec<(f32, f32, f32)> = Vec::new();
        // The loupe: where it is, the point it was placed from, and the
        // word the passage ended on when last drawn.
        let mut loupe_at: Option<Rect> = None;
        let mut loupe_from = (0.0, 0.0);
        let mut drawn_end: Option<usize> = None;
        flash = false;
        ahead.reading(&book, chapter);
        // Where a light gauge is showing, to clear at the next touch.
        let mut gauge: Option<Rect> = None;
        let step = 'reading: loop {
            // On the page, a pen that moves writes.
            screen.ink(true);
            let gesture = screen.next();
            screen.ink(false);
            let page_now = &t.pages[reader.page];
            if let Gesture::Ink(..) | Gesture::InkUp(..) = gesture {
                let tool = screen.tool();
                let up = matches!(gesture, Gesture::InkUp(..));
                let points: Vec<(f32, f32, f32)> = screen
                    .take_ink()
                    .iter()
                    .map(|&(x, y, p)| {
                        // The highlighter's trail is a guide, finer than ink.
                        let r = if tool == Tool::Highlighter {
                            1.0
                        } else {
                            pen_radius(p)
                        };
                        (x as f32, y as f32, r)
                    })
                    .collect();
                if tool == Tool::Eraser {
                    // What the eraser passes over goes: strokes it comes near,
                    // and underlines whose words it touches.
                    let reach = tools.profile.mm(2.5);
                    let near =
                        |x: f32, y: f32| points.iter().any(|p| (p.0 - x).hypot(p.1 - y) <= reach);
                    let mut gone: Vec<String> = Vec::new();
                    {
                        let marks = tools.marks.borrow();
                        for (id, line) in strokes(
                            page_now,
                            &numbered,
                            reader.page,
                            &marks,
                            path,
                            chapter,
                            width,
                        ) {
                            if line.iter().any(|q| near(q.0, q.1)) {
                                gone.push(id);
                            }
                        }
                        let numbers = &numbered.on_page[reader.page];
                        for u in marks.in_chapter(path, chapter) {
                            let Some((s, e)) = numbered.span(u) else {
                                continue;
                            };
                            let touched = page_now.words.iter().zip(numbers).any(|(w, n)| {
                                (s..=e).contains(n)
                                    && points.iter().any(|p| {
                                        p.0 >= w.x0 - reach
                                            && p.0 <= w.x1 + reach
                                            && p.1 >= w.y0
                                            && p.1 <= w.y1 + reach
                                    })
                            });
                            if touched {
                                gone.push(u.id.clone());
                            }
                        }
                    }
                    if !gone.is_empty() {
                        for id in &gone {
                            if let Err(e) = tools.marks.borrow_mut().remove(id) {
                                eprintln!("erase: {e}");
                            }
                        }
                        eprintln!("erased {}", gone.len());
                        shown = draw(
                            screen,
                            &mut renderer,
                            &t.pages[reader.page],
                            false,
                            &rules(reader.page),
                        );
                    }
                    continue;
                }
                // From the last point drawn, so the line has no gaps.
                let mut piece: Vec<_> = stroke.last().copied().into_iter().collect();
                piece.extend(&points);
                if let Some(r) = ink_into(&mut shown, width, height, &piece) {
                    let block = cut(&shown, width, &r);
                    let (w, h) = ((r.x1 - r.x0) as u32, (r.y1 - r.y0) as u32);
                    screen.draw_fast(&block, w, h, r.x0 as u32, r.y0 as u32);
                }
                stroke.extend(points);
                if up && !stroke.is_empty() && tool == Tool::Highlighter {
                    // The words the stroke ran through, or just under:
                    // underlined, from the first to the last.
                    let hit: Vec<usize> = page_now
                        .words
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| {
                            let below = (w.y1 - w.y0) * 0.5;
                            stroke.iter().any(|p| {
                                p.0 >= w.x0 && p.0 <= w.x1 && p.1 >= w.y0 && p.1 <= w.y1 + below
                            })
                        })
                        .map(|(k, _)| k)
                        .collect();
                    if let (Some(&a), Some(&b)) = (hit.first(), hit.last()) {
                        let numbers = &numbered.on_page[reader.page];
                        let u =
                            underline_of(&book, path, chapter, page_now, numbers, &numbered, a, b);
                        match tools.marks.borrow_mut().underline(u) {
                            Ok(()) => eprintln!(
                                "underlined words {} to {} with the pen",
                                numbers[a], numbers[b]
                            ),
                            Err(e) => eprintln!("underline: {e}"),
                        }
                    }
                    stroke.clear();
                    // The trail gone, the underline drawn.
                    shown = draw(
                        screen,
                        &mut renderer,
                        &t.pages[reader.page],
                        false,
                        &rules(reader.page),
                    );
                    continue;
                }
                if up && !stroke.is_empty() {
                    // Anchored to the word nearest its middle, measured in
                    // that word's line heights.
                    let (sx, sy) = stroke
                        .iter()
                        .fold((0.0, 0.0), |(a, b), p| (a + p.0, b + p.1));
                    let n = stroke.len() as f32;
                    if let Some(k) = nearest_word(page_now, sx / n, sy / n) {
                        let w = &page_now.words[k];
                        let lh = (w.y1 - w.y0).max(1.0);
                        let round = |v: f32| (v * 1000.0).round() / 1000.0;
                        // In a margin, if its middle is outside the text.
                        let (left, right) = text_edges(page_now, width);
                        let margin = if sx / n > right.0 {
                            Some(("right", right))
                        } else if sx / n < left.0 + left.1 {
                            Some(("left", left))
                        } else {
                            None
                        };
                        // In a margin, the first point across as a share of
                        // it, and the rest from that point, so the stroke
                        // keeps its shape.
                        let first = stroke[0].0;
                        let across = |k: usize, x: f32| match margin {
                            Some((_, (from, span))) if k == 0 => (x - from) / span,
                            Some(_) => (x - first) / lh,
                            None => (x - w.x0) / lh,
                        };
                        let points = stroke
                            .iter()
                            .enumerate()
                            .map(|(k, (x, y, r))| {
                                [round(across(k, *x)), round((y - w.y0) / lh), round(r / lh)]
                            })
                            .collect();
                        let done = tools.marks.borrow_mut().ink(Ink {
                            id: String::new(),
                            time: 0,
                            book: book.title(),
                            path: path.to_string(),
                            chapter,
                            anchor: numbered.anchors[numbered.on_page[reader.page][k]].clone(),
                            margin: margin.map(|m| m.0.to_string()),
                            points,
                        });
                        match done {
                            Ok(()) => eprintln!("ink: {} points by {:?}", stroke.len(), w.text),
                            Err(e) => eprintln!("ink: {e}"),
                        }
                    }
                    stroke.clear();
                }
                continue;
            }
            let numbers = &numbered.on_page[reader.page];
            // The grey a word looked up has, over a passage, line by line.
            let mark = |chosen: (usize, usize)| -> Vec<Rect> {
                let (a, b) = (chosen.0.min(chosen.1), chosen.0.max(chosen.1));
                runs(page_now, |k| (a..=b).contains(&k))
                    .into_iter()
                    .map(|r| run_box(page_now, r))
                    .collect()
            };
            // The rows from `boxes` and those last drawn over, from `pixels`.
            let mut redraw_rows = |screen: &mut dyn Platform, pixels: &[u8], boxes: &[Rect]| {
                let top = boxes
                    .iter()
                    .map(|b| b.y0.max(0.0) as u32)
                    .min()
                    .unwrap_or(height);
                let bottom = boxes
                    .iter()
                    .map(|b| b.y1 as u32)
                    .max()
                    .unwrap_or(0)
                    .min(height);
                let (top, bottom) = match marked_rows.take() {
                    Some((t0, b0)) => (top.min(t0), bottom.max(b0)),
                    None => (top, bottom),
                };
                if bottom > top {
                    let rows = &pixels[(top * width) as usize..(bottom * width) as usize];
                    screen.draw(rows, width, bottom - top, 0, top, false);
                }
                (top, bottom)
            };
            match gesture {
                Gesture::Press(x, y) => {
                    let k = crate::reader::word_at(page_now, x as f32, y as f32);
                    chosen = k.map(|k| (k, k));
                    editing = None;
                    loupe_at = None;
                    drawn_end = None;
                    // Pressed on an underline: the whole of it, to take
                    // away, rather than a new one.
                    if let Some(k) = k {
                        let n = numbers[k];
                        let found = tools
                            .marks
                            .borrow()
                            .in_chapter(path, chapter)
                            .find_map(|u| {
                                let (s, e) = numbered.span(u)?;
                                (s..=e).contains(&n).then(|| (u.id.clone(), s, e))
                            });
                        if let Some((id, s, e)) = found {
                            let on: Vec<usize> = (0..numbers.len())
                                .filter(|&i| (s..=e).contains(&numbers[i]))
                                .collect();
                            chosen = Some((on[0], on[on.len() - 1]));
                            editing = Some((id, k));
                        }
                    }
                }
                Gesture::Drag(x, y) if editing.is_none() => {
                    if let (Some(c), Some(k)) = (chosen, nearest_word(page_now, x as f32, y as f32))
                    {
                        chosen = Some((c.0, k));
                    }
                }
                _ => {}
            }
            if let (Gesture::Press(x, y) | Gesture::Drag(x, y), Some(c)) = (gesture, chosen) {
                let (fx, fy) = (x as f32, y as f32);
                let pressing = matches!(gesture, Gesture::Press(..));
                let started = Instant::now();
                // The loupe stays put unless the finger comes to it.
                let reach = tools.profile.mm(4.0);
                let near = loupe_at.is_some_and(|r: Rect| {
                    fx > r.x0 - reach && fx < r.x1 + reach && fy > r.y0 - reach && fy < r.y1 + reach
                });
                // Nothing to draw until the passage ends on another word:
                // e-ink can't keep up with every move.
                if !pressing && drawn_end == Some(c.1) && !near {
                    continue;
                }
                drawn_end = Some(c.1);
                let mut boxes = mark(c);
                let mut marked = highlight(&shown, width, height, &boxes);
                // An underline held is grey to its ends, past the finger:
                // no loupe needed.
                if editing.is_some() {
                    marked_rows = Some(redraw_rows(screen, &marked, &boxes));
                    continue;
                }
                if pressing || near {
                    loupe_from = (fx, fy);
                }
                let (lp, lr) = ui.loupe(&page_now.words[c.1].text, loupe_from.0, loupe_from.1);
                let (lw, lh) = ((lr.x1 - lr.x0) as u32, (lr.y1 - lr.y0) as u32);
                let lens = chrome.page(&lp, lw, lh);
                paste(&mut marked, width, height, &lens, &lr);
                // Where it was before, if it moved or narrowed, goes back.
                if let Some(old) = loupe_at.filter(|o| *o != lr) {
                    boxes.push(old);
                }
                let rows = redraw_rows(screen, &marked, &boxes);
                // The loupe is black on white: the fast refresh will do.
                screen.draw_fast(&lens, lw, lh, lr.x0 as u32, lr.y0 as u32);
                marked_rows = Some((rows.0.min(lr.y0 as u32), rows.1.max(lr.y1 as u32)));
                loupe_at = Some(lr);
                eprintln!("{gesture:?}: loupe drawn in {:?}", started.elapsed());
                continue;
            }
            let mut forced = None;
            let held = matches!(gesture, Gesture::Release(..) | Gesture::Hold(..));
            if let (true, Some((id, k)), Some(c)) = (
                held && editing.is_some() && chosen.is_some(),
                editing.take(),
                chosen,
            ) {
                chosen = None;
                // An underline held: grey still, with what can be done
                // with it beside it.
                let mut boxes = mark(c);
                let above = boxes.iter().map(|b| b.y0).fold(f32::MAX, f32::min);
                let below = boxes.iter().map(|b| b.y1).fold(0.0, f32::max);
                let mut marked = highlight(&shown, width, height, &boxes);
                let (bp, br, hits) = ui.choices(&["remove underline", "look up"], above, below);
                let (bw, bh) = ((br.x1 - br.x0) as u32, (br.y1 - br.y0) as u32);
                let bar = chrome.page(&bp, bw, bh);
                paste(&mut marked, width, height, &bar, &br);
                boxes.push(br);
                let rows = redraw_rows(screen, &marked, &boxes);
                let choice = match screen.next() {
                    Gesture::Tap(x, y) => hits.iter().position(|r| r.contains(x as f32, y as f32)),
                    Gesture::Power => break Step::Leave(Leave::Close),
                    _ => None,
                };
                if choice == Some(0) {
                    match tools.marks.borrow_mut().remove(&id) {
                        Ok(()) => eprintln!("underline removed"),
                        Err(e) => eprintln!("underline: {e}"),
                    }
                    shown = draw(
                        screen,
                        &mut renderer,
                        &t.pages[reader.page],
                        false,
                        &rules(reader.page),
                    );
                    continue;
                }
                // The page as it was, then the lookup, if that was chosen.
                let page_rows = &shown[(rows.0 * width) as usize..(rows.1 * width) as usize];
                screen.draw(page_rows, width, rows.1 - rows.0, 0, rows.0, false);
                if choice != Some(1) {
                    continue;
                }
                forced = Some(Effect::Look(k));
            }
            if let Gesture::Release(..) = gesture {
                marked_rows = None;
                if let Some((a, b)) = chosen.take() {
                    let u = underline_of(&book, path, chapter, page_now, numbers, &numbered, a, b);
                    let (s, e) = (numbers[a.min(b)], numbers[a.max(b)]);
                    let done = tools.marks.borrow_mut().underline(u);
                    match done {
                        Ok(()) => eprintln!("underlined words {s} to {e}"),
                        Err(e) => eprintln!("underline: {e}"),
                    }
                }
                shown = draw(
                    screen,
                    &mut renderer,
                    &t.pages[reader.page],
                    false,
                    &rules(reader.page),
                );
                continue;
            }
            // Pressed but never moved: a hold, which looks the word up. The
            // loupe and the grey go first.
            if chosen.take().is_some() {
                redraw_rows(screen, &shown, &[]);
            }
            marked_rows = None;
            let effect = forced.unwrap_or_else(|| reader.act(gesture));
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
                    shown = draw(
                        screen,
                        &mut renderer,
                        &t.pages[reader.page],
                        flash,
                        &rules(reader.page),
                    );
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
                    // The pack it was found in, and each reading shown, by
                    // headword and part of speech: for the hoard.
                    let mut pack = String::new();
                    let mut found: Vec<(String, String)> = Vec::new();
                    let blocks = match tools.lexicon.look_up(&word, &lang) {
                        Some((p, readings)) => {
                            pack = p.to_string();
                            found = readings
                                .iter()
                                .take(lexicon::SHOWN)
                                .map(|r| (r.entry.word.clone(), r.entry.pos.clone()))
                                .collect();
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
                    let (w_px, h) = (area.width as u32, area.height as u32);
                    let w = w_px;
                    let right = area.width - tools.profile.mm(tools.profile.margins.right);
                    // The sentence the word is in here, and where in it.
                    let sentence_here = || {
                        hoard::sentence(&book.blocks(chapter), &block, nth, &printed)
                            .unwrap_or_else(|| (printed.clone(), 0))
                    };
                    let mut here: Option<(String, usize)> = None;
                    let mut at = 0;
                    loop {
                        let mut pixels = render::panel(&mut chrome, &pages[at], w, h);
                        // Each reading's mark, flush right on its headline:
                        // "hoard", or "in the hoard" for the one kept here.
                        let kept = {
                            let hoard = tools.hoard.borrow();
                            let kept = hoard.kept_in(&printed, path, chapter, &block);
                            // Only if it might be kept here: the sentence
                            // means reading the chapter's text.
                            if !kept.is_empty() && here.is_none() {
                                here = Some(sentence_here());
                            }
                            kept.into_iter()
                                .find(|k| here.as_ref() == Some(&(k.sentence.clone(), k.at)))
                                .map(|k| (k.id.clone(), k.head.clone(), k.pos.clone()))
                        };
                        let (ascent, descent) = fonts.ascent_descent(1.0);
                        let mut marks: Vec<(Rect, usize)> = Vec::new();
                        for (n, (head, pos)) in found.iter().enumerate() {
                            let id = format!("reading-{n}");
                            let line: Vec<_> =
                                pages[at].words.iter().filter(|w| w.block == id).collect();
                            let Some(first) = line.first() else { continue };
                            let last = line.last().unwrap_or(first);
                            let is_kept =
                                kept.as_ref().is_some_and(|(_, h, p)| h == head && p == pos);
                            let text = if is_kept { "in the hoard" } else { "hoard" };
                            // The headline's first line, or its last if the
                            // first runs too far right to leave room.
                            for w in [first, last] {
                                let baseline = w.y0 + (w.y1 - w.y0) * ascent / (ascent + descent);
                                let (mark, hit) = ui.label(text, is_kept, right, baseline);
                                let gap = tools.profile.mm(3.0);
                                let room = line
                                    .iter()
                                    .filter(|o| (o.y0 - w.y0).abs() < 1.0)
                                    .all(|o| o.x1 + gap < hit.x0 + tools.profile.mm(4.0));
                                if room {
                                    let over = chrome.page(&mark, w_px, h);
                                    for (p, o) in pixels.iter_mut().zip(&over) {
                                        *p = (*p).min(*o);
                                    }
                                    marks.push((hit, n));
                                    break;
                                }
                            }
                        }
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
                                    if let Some(&(_, n)) =
                                        marks.iter().find(|(r, _)| r.contains(px, py))
                                    {
                                        let (head, pos) = &found[n];
                                        let mut hoard = tools.hoard.borrow_mut();
                                        // One reading kept here: another's
                                        // mark moves it, its own lets it go.
                                        let mut done = Ok(());
                                        let mut again = true;
                                        if let Some((id, h, p)) = &kept {
                                            done = hoard.let_go(id);
                                            again = !(h == head && p == pos);
                                        }
                                        if again && done.is_ok() {
                                            let (sentence, at) =
                                                here.get_or_insert_with(sentence_here).clone();
                                            done = hoard.keep(Kept {
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
                                            });
                                        }
                                        match done {
                                            Ok(()) => eprintln!(
                                                "hoard: {head:?} {pos} {}",
                                                if again { "kept" } else { "let go" }
                                            ),
                                            Err(e) => eprintln!("hoard: {e}"),
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
                                    shown = draw(
                                        screen,
                                        &mut renderer,
                                        &t.pages[reader.page],
                                        true,
                                        &rules(reader.page),
                                    )
                                }
                            }
                        }
                        Some(Action::Contents) => {
                            match contents(screen, &book, ui, &mut chrome, chapter, &search_text) {
                                Some(Ok((k, at))) => break Step::Jump(k, at),
                                Some(Err(leave)) => break Step::Leave(leave),
                                None => {
                                    shown = draw(
                                        screen,
                                        &mut renderer,
                                        &t.pages[reader.page],
                                        true,
                                        &rules(reader.page),
                                    )
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
            Step::Jump(k, at) => {
                chapter = k;
                page = 0;
                find_anchor = at;
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
    text: &crate::search::Text,
) -> Option<Result<(usize, Arrival), Leave>> {
    let (width, height) = screen.size();
    let entries = book.contents();
    // The entry for the chapter being read: the last one at or before it.
    let at = entries
        .iter()
        .rposition(|(_, _, c)| *c <= current)
        .unwrap_or(0);
    // First, a way to search the book; then its contents.
    let search_row = Row {
        title: "Search this book".into(),
        detail: Vec::new(),
        cover: None,
        progress: None,
        count: None,
        mark: false,
        action: Action::Search,
        depth: 0,
    };
    let rows: Vec<Row> = std::iter::once(search_row)
        .chain(
            entries
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
                }),
        )
        .collect();
    let per_page = ui
        .list("Contents", &rows, 0, true)
        .hits
        .len()
        .saturating_sub(1)
        .max(1);
    let mut page = (at + 1) / per_page;
    // What was last searched for, to start from again.
    let mut query = String::new();
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
                    Some(Action::Chapter(k)) => return Some(Ok((*k, None))),
                    Some(Action::Back) => return None,
                    Some(Action::Search) => {
                        match search_book(screen, book, ui, chrome, &mut query, text) {
                            Some(found) => return Some(found),
                            None => break,
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

/// What typing came to.
enum Typed {
    Done,
    Back,
    Close,
}

/// Typing into `text` on the keyboard, under a header `title`, until search
/// is tapped, or ‹.
fn type_in(
    screen: &mut dyn Platform,
    ui: &Ui,
    chrome: &mut Renderer,
    title: &str,
    text: &mut String,
) -> Typed {
    let (width, height) = screen.size();
    let mut shift = false;
    // The whole screen at first and when the keys change; else only the
    // line of text.
    let mut whole = true;
    // A key tapped, shown pressed until the screen catches up.
    let mut pressed: Option<Rect> = None;
    loop {
        let s = ui.typing(title, text, shift);
        let pixels = chrome.page(&s.page, width, height);
        if whole {
            screen.draw(&pixels, width, height, 0, 0, true);
        } else {
            let (top, bottom) = ui.typing_band();
            let (top, bottom) = (top as u32, (bottom as u32).min(height));
            let rows = &pixels[(top * width) as usize..(bottom * width) as usize];
            screen.draw(rows, width, bottom - top, 0, top, false);
            // The key back as it was.
            if let Some(r) = pressed.take() {
                let key = cut(&pixels, width, &r);
                screen.draw_fast(
                    &key,
                    (r.x1 - r.x0) as u32,
                    (r.y1 - r.y0) as u32,
                    r.x0 as u32,
                    r.y0 as u32,
                );
            }
        }
        whole = false;
        loop {
            let hit = match screen.next() {
                Gesture::Power => return Typed::Close,
                Gesture::Tap(x, y) => s
                    .hits
                    .iter()
                    .find(|(r, _)| r.contains(x as f32, y as f32))
                    .cloned(),
                _ => None,
            };
            // A key goes down at once, white on black, before the text it
            // types can be drawn: e-ink is slow to show a letter, and space
            // or delete show little.
            if let Some((r, Action::Key(_) | Action::Delete | Action::Shift | Action::Done)) = &hit
            {
                let r = Rect {
                    x0: r.x0.round(),
                    y0: r.y0.round() + 2.0,
                    x1: r.x1.round(),
                    y1: r.y1.round() - 1.0,
                };
                let key: Vec<u8> = cut(&pixels, width, &r).iter().map(|p| 255 - p).collect();
                screen.draw_fast(
                    &key,
                    (r.x1 - r.x0) as u32,
                    (r.y1 - r.y0) as u32,
                    r.x0 as u32,
                    r.y0 as u32,
                );
                pressed = Some(r);
            }
            let action = hit.map(|(_, a)| a);
            match action {
                Some(Action::Key(c)) => {
                    text.push(c);
                    if shift {
                        shift = false;
                        whole = true;
                    }
                }
                Some(Action::Delete) => {
                    text.pop();
                }
                Some(Action::Clear) => text.clear(),
                Some(Action::Shift) => {
                    shift = !shift;
                    whole = true;
                }
                Some(Action::Done) if !text.trim().is_empty() => return Typed::Done,
                Some(Action::Back) => return Typed::Back,
                _ => continue,
            }
            break;
        }
    }
}

/// Searching the book: type, then the places found, each with its chapter
/// and the words around it, the words asked for in italic. A place chosen,
/// `Some(Ok)`; Wordhord closed, `Some(Err)`; back, `None`.
fn search_book(
    screen: &mut dyn Platform,
    book: &Book,
    ui: &Ui,
    chrome: &mut Renderer,
    query: &mut String,
    text: &crate::search::Text,
) -> Option<Result<(usize, Arrival), Leave>> {
    let (width, height) = screen.size();
    let entries = book.contents();
    loop {
        match type_in(screen, ui, chrome, "Search", query) {
            Typed::Done => {}
            Typed::Back => return None,
            Typed::Close => return Some(Err(Leave::Close)),
        }
        let s = ui.centred_note("Search", &["Searching…".to_string()]);
        let pixels = chrome.page(&s.page, width, height);
        screen.draw(&pixels, width, height, 0, 0, false);
        let started = Instant::now();
        let found = crate::search::search(book, text, query, 200);
        eprintln!(
            "search {query:?}: {} found in {:?}",
            found.len(),
            started.elapsed()
        );
        let rows: Vec<Row> = found
            .iter()
            .enumerate()
            .map(|(k, f)| Row {
                title: entries
                    .iter()
                    .rfind(|c| c.2 <= f.chapter)
                    .map_or_else(|| format!("Chapter {}", f.chapter + 1), |c| c.0.clone()),
                detail: vec![
                    Part::Plain(format!("{} ", f.before)),
                    Part::Italic(f.words.clone()),
                    Part::Plain(format!(" {}", f.after)),
                ],
                cover: None,
                progress: None,
                count: None,
                mark: false,
                action: Action::Found(k),
                depth: 0,
            })
            .collect();
        let title = if found.is_empty() {
            format!("Nothing for “{query}”")
        } else {
            format!("“{query}”")
        };
        let mut page = 0;
        'results: loop {
            let s = ui.list(&title, &rows, page, true);
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
                        Some(Action::Found(k)) => {
                            let f = &found[*k];
                            let words = f.words.split_whitespace().map(str::to_string).collect();
                            return Some(Ok((f.chapter, Some((f.anchor.clone(), words)))));
                        }
                        // Back to the keyboard, the words still there.
                        Some(Action::Back) => break 'results,
                        _ => {}
                    },
                    _ => {}
                }
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
fn draw(
    screen: &mut dyn Platform,
    renderer: &mut Renderer,
    page: &Page,
    flash: bool,
    decor: &Decor,
) -> Vec<u8> {
    let (width, height) = screen.size();
    let started = Instant::now();
    let mut pixels = renderer.page(page, width, height);
    rule(&mut pixels, width, height, &decor.rules);
    for stroke in &decor.strokes {
        ink_into(&mut pixels, width, height, stroke);
    }
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
    draw(screen, &mut renderer, &page, true, &Decor::default());
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
