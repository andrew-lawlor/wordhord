//! The library's screens, drawn as pages: text, rules and covers, and the
//! places to tap on them.
//!
//! - Home: the books read lately, as covers with how far through each is;
//!   then the shelves: Authors, Series, Tags, All.
//! - A shelf: its authors, series or tags, or all books, as a list.
//! - A list of books: a small cover, the title, the author and series, and
//!   a bar for how far through.
//!
//! Lists page with a swipe. The ‹ at the top left goes back.

use wordhord_typeset::fonts::{Face, Fonts};
use wordhord_typeset::page::{Page, PageImage, Rule, Run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    Authors,
    Series,
    Tags,
    All,
}

impl Shelf {
    pub fn name(self) -> &'static str {
        match self {
            Shelf::Authors => "Authors",
            Shelf::Series => "Series",
            Shelf::Tags => "Tags",
            Shelf::All => "All books",
        }
    }
}

/// What a tap on a place does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Shelf(Shelf),
    /// One author, series or tag of a shelf, by its place in the shelf.
    Group(Shelf, usize),
    /// A book, by its path.
    Open(String),
    /// A chapter of the book being read.
    Chapter(usize),
    Back,
    /// The reader's bar: back to the library, or to the contents.
    Library,
    Contents,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    fn contains(&self, x: f32, y: f32) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }
}

pub struct Screen {
    pub page: Page,
    pub hits: Vec<(Rect, Action)>,
    /// How many pages the list has.
    pub pages: usize,
}

impl Screen {
    /// What a tap at (x, y) does, if anything.
    pub fn hit(&self, x: f32, y: f32) -> Option<&Action> {
        self.hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, a)| a)
    }
}

/// A row in a list.
pub struct Row {
    pub title: String,
    pub detail: String,
    /// A book's cover: its path in the library, and its size.
    pub cover: Option<(String, (u32, u32))>,
    /// How far through, for a book that's been opened.
    pub progress: Option<f32>,
    pub action: Action,
    /// Indent steps, for contents.
    pub depth: usize,
}

/// A book on the Continue shelf.
pub struct Recent {
    pub path: String,
    pub title: String,
    pub cover: Option<(u32, u32)>,
    pub progress: f32,
}

pub struct Ui<'f> {
    fonts: &'f Fonts,
    w: f32,
    h: f32,
    ppi: f32,
}

/// Covers are asked for by this prefix on their path, so the renderer's
/// image reader knows to look in the library.
pub const COVER: &str = "cover:";

impl<'f> Ui<'f> {
    pub fn new(fonts: &'f Fonts, width: u32, height: u32, ppi: f32) -> Ui<'f> {
        Ui {
            fonts,
            w: width as f32,
            h: height as f32,
            ppi,
        }
    }

    fn mm(&self, mm: f32) -> f32 {
        mm * self.ppi / 25.4
    }

    fn pt(&self, pt: f32) -> f32 {
        pt * self.ppi / 72.0
    }

    fn margin(&self) -> f32 {
        self.mm(9.0)
    }

    /// Text at a baseline, cut with an ellipsis to fit `max` pixels. Returns
    /// its width.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        page: &mut Page,
        text: &str,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        max: f32,
    ) -> f32 {
        let width = |t: &str| -> f32 {
            self.fonts
                .shape(t, face, size, &[])
                .iter()
                .map(|s| s.width)
                .sum()
        };
        let mut shown = text.to_string();
        if width(&shown) > max {
            let chars: Vec<char> = text.chars().collect();
            let mut n = chars.len();
            while n > 0 {
                n -= 1;
                shown = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
                if width(&shown) <= max {
                    break;
                }
            }
        }
        let mut cx = x;
        for s in self.fonts.shape(&shown, face, size, &[]) {
            page.runs.push(Run {
                font: s.font,
                size: s.size,
                x: cx,
                y,
                glyphs: s.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
            });
            cx += s.width;
        }
        cx - x
    }

    fn width(&self, text: &str, face: Face, size: f32) -> f32 {
        self.fonts
            .shape(text, face, size, &[])
            .iter()
            .map(|s| s.width)
            .sum()
    }

    fn rule(&self, page: &mut Page, x: f32, y: f32, width: f32, height: f32, grey: u8) {
        page.rules.push(Rule {
            x,
            y,
            width,
            height,
            grey,
        });
    }

    /// A cover fitted in a box, centred, or a framed blank without one.
    #[allow(clippy::too_many_arguments)]
    fn cover(
        &self,
        page: &mut Page,
        path: &str,
        size: Option<(u32, u32)>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        match size {
            Some((cw, ch)) if cw > 0 && ch > 0 => {
                let scale = (w / cw as f32).min(h / ch as f32);
                let (dw, dh) = (cw as f32 * scale, ch as f32 * scale);
                page.images.push(PageImage {
                    src: format!("{COVER}{path}"),
                    x: x + (w - dw) / 2.0,
                    y: y + (h - dh),
                    width: dw.round(),
                    height: dh.round(),
                });
            }
            _ => {
                let t = self.mm(0.3).max(1.0);
                self.rule(page, x, y, w, t, 0);
                self.rule(page, x, y + h - t, w, t, 0);
                self.rule(page, x, y, t, h, 0);
                self.rule(page, x + w - t, y, t, h, 0);
            }
        }
    }

    /// A progress bar: a thin line, filled `share` of its length.
    fn bar(&self, page: &mut Page, x: f32, y: f32, w: f32, share: f32) {
        let t = self.mm(0.4).max(2.0);
        self.rule(page, x, y, w, t, 187);
        self.rule(page, x, y - t / 2.0, w * share.clamp(0.0, 1.0), t * 2.0, 0);
    }

    /// The title along the top, with ‹ to go back and the page count.
    fn header(&self, s: &mut Screen, title: &str, back: bool, page: usize) -> f32 {
        let m = self.margin();
        let size = self.pt(14.0);
        let baseline = self.mm(9.0) + size;
        let tw = self.width(title, Face::Regular, size).min(self.w - 4.0 * m);
        self.text(
            &mut s.page,
            title,
            Face::Regular,
            size,
            (self.w - tw) / 2.0,
            baseline,
            self.w - 4.0 * m,
        );
        if back {
            self.text(
                &mut s.page,
                "‹",
                Face::Regular,
                size * 1.4,
                m,
                baseline,
                m * 2.0,
            );
            s.hits.push((
                Rect {
                    x0: 0.0,
                    y0: 0.0,
                    x1: m * 3.0,
                    y1: baseline + size,
                },
                Action::Back,
            ));
        }
        if s.pages > 1 {
            let small = self.pt(9.0);
            let label = format!("{} / {}", page + 1, s.pages);
            let lw = self.width(&label, Face::Regular, small);
            self.text(
                &mut s.page,
                &label,
                Face::Regular,
                small,
                self.w - m - lw,
                baseline,
                m * 3.0,
            );
        }
        let y = baseline + size * 0.8;
        self.rule(
            &mut s.page,
            m,
            y,
            self.w - 2.0 * m,
            self.mm(0.3).max(1.0),
            0,
        );
        y + self.mm(3.0)
    }

    /// Home: the books read lately, then the shelves with their sizes.
    pub fn home(&self, recent: &[Recent], shelves: [(Shelf, usize); 4]) -> Screen {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            pages: 1,
        };
        let m = self.margin();
        let mut y = self.header(&mut s, "Wordhord", false, 0);
        let label = self.pt(10.0);
        if !recent.is_empty() {
            y += label;
            self.text(&mut s.page, "Continue", Face::Italic, label, m, y, self.w);
            y += self.mm(3.0);
            let gap = self.mm(5.0);
            let cw = (self.w - 2.0 * m - 3.0 * gap) / 4.0;
            let ch = cw * 1.5;
            let small = self.pt(8.5);
            for (k, r) in recent.iter().take(4).enumerate() {
                let x = m + k as f32 * (cw + gap);
                self.cover(&mut s.page, &r.path, r.cover, x, y, cw, ch);
                self.text(
                    &mut s.page,
                    &r.title,
                    Face::Regular,
                    small,
                    x,
                    y + ch + small * 1.4,
                    cw,
                );
                self.bar(&mut s.page, x, y + ch + small * 2.3, cw, r.progress);
                s.hits.push((
                    Rect {
                        x0: x,
                        y0: y,
                        x1: x + cw,
                        y1: y + ch + small * 2.6,
                    },
                    Action::Open(r.path.clone()),
                ));
            }
            y += ch + small * 2.6 + self.mm(8.0);
        }
        let size = self.pt(13.0);
        let row = self.mm(16.0);
        for (shelf, count) in shelves {
            if count == 0 {
                continue;
            }
            let baseline = y + (row + size * 0.6) / 2.0;
            self.text(
                &mut s.page,
                shelf.name(),
                Face::Regular,
                size,
                m,
                baseline,
                self.w,
            );
            let n = count.to_string();
            let nw = self.width(&n, Face::Regular, self.pt(10.0));
            self.text(
                &mut s.page,
                &n,
                Face::Regular,
                self.pt(10.0),
                self.w - m - nw,
                baseline,
                self.w,
            );
            self.rule(&mut s.page, m, y + row, self.w - 2.0 * m, 1.0, 153);
            s.hits.push((
                Rect {
                    x0: 0.0,
                    y0: y,
                    x1: self.w,
                    y1: y + row,
                },
                Action::Shelf(shelf),
            ));
            y += row;
        }
        s
    }

    /// A list: a page of its rows, under a title.
    pub fn list(&self, title: &str, rows: &[Row], page: usize, back: bool) -> Screen {
        let books = rows.iter().any(|r| r.cover.is_some());
        let row_h = if books { self.mm(22.0) } else { self.mm(13.0) };
        let top_guess = self.mm(9.0) + self.pt(14.0) * 1.8 + self.mm(3.0);
        let per_page = (((self.h - top_guess - self.mm(8.0)) / row_h).floor() as usize).max(1);
        let pages = rows.len().div_ceil(per_page).max(1);
        let page = page.min(pages - 1);
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            pages,
        };
        let m = self.margin();
        let mut y = self.header(&mut s, title, back, page);
        let size = self.pt(11.0);
        let small = self.pt(8.5);
        for r in rows.iter().skip(page * per_page).take(per_page) {
            let mut x = m + r.depth as f32 * self.mm(5.0);
            if books {
                let (cw, ch) = (self.mm(12.0), self.mm(18.0));
                if let Some((path, size)) = &r.cover {
                    let size = Some(*size).filter(|s| s.0 > 0);
                    self.cover(&mut s.page, path, size, x, y + self.mm(2.0), cw, ch);
                }
                x += cw + self.mm(5.0);
            }
            let width = self.w - m - x;
            let first = if books {
                y + self.mm(2.0) + size
            } else {
                y + (row_h + size * 0.6) / 2.0 - small * 0.6
            };
            self.text(&mut s.page, &r.title, Face::Regular, size, x, first, width);
            if !r.detail.is_empty() {
                self.text(
                    &mut s.page,
                    &r.detail,
                    Face::Italic,
                    small,
                    x,
                    first + small * 1.6,
                    width,
                );
            }
            if let Some(p) = r.progress {
                self.bar(&mut s.page, x, first + small * 3.2, self.mm(30.0), p);
            }
            self.rule(&mut s.page, m, y + row_h, self.w - 2.0 * m, 1.0, 187);
            s.hits.push((
                Rect {
                    x0: 0.0,
                    y0: y,
                    x1: self.w,
                    y1: y + row_h,
                },
                r.action.clone(),
            ));
            y += row_h;
        }
        s
    }

    /// The bar over the top of a page while reading: back to the library,
    /// the book's title, and the contents.
    pub fn reading_bar(&self, title: &str) -> (Screen, u32) {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            pages: 1,
        };
        let m = self.margin();
        let size = self.pt(11.0);
        let height = self.mm(16.0);
        let baseline = (height + size * 0.6) / 2.0;
        let lw = self.text(
            &mut s.page,
            "‹ Library",
            Face::Regular,
            size,
            m,
            baseline,
            self.w / 3.0,
        );
        let cw = self.width("Contents", Face::Regular, size);
        self.text(
            &mut s.page,
            "Contents",
            Face::Regular,
            size,
            self.w - m - cw,
            baseline,
            self.w / 3.0,
        );
        let tw = self
            .width(title, Face::Italic, size * 0.9)
            .min(self.w - 2.0 * m - lw - cw - 2.0 * m);
        self.text(
            &mut s.page,
            title,
            Face::Italic,
            size * 0.9,
            (self.w - tw) / 2.0,
            baseline,
            tw.max(1.0),
        );
        self.rule(&mut s.page, 0.0, height - 3.0, self.w, 3.0, 0);
        s.hits.push((
            Rect {
                x0: 0.0,
                y0: 0.0,
                x1: m + lw + m,
                y1: height,
            },
            Action::Library,
        ));
        s.hits.push((
            Rect {
                x0: self.w - m - cw - m,
                y0: 0.0,
                x1: self.w,
                y1: height,
            },
            Action::Contents,
        ));
        (s, height as u32)
    }
}
