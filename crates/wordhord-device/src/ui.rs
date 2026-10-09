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
    /// The reader's bar: back to the library, to the type, or to the
    /// contents.
    Library,
    Type,
    Contents,
    /// The hoard: its words, and one word's page by its place in them.
    Hoard,
    Word(usize),
    /// The commonplace book: every underline; and one passage of it, by
    /// its place there, to open the book at.
    Commonplace,
    /// A book in the commonplace book, by its place in the index.
    Passages(usize),
    Passage(usize),
    /// Unlocking the hoard: begin, show the entry, grade it 1 to 4.
    Unlock,
    Reveal,
    Grade(u8),
    /// On the type screen: the nth choice of a row.
    Choose(Choice, usize),
    /// Close Wordhord, back to the Kobo's own reader.
    Close,
    /// The keyboard: a letter, shift, delete, and done.
    Key(char),
    Shift,
    Delete,
    Done,
    /// In the contents: search the book; and a place found, by its place
    /// in the results.
    Search,
    Found(usize),
    /// Search the library, from home; and the commonplace book, from its
    /// index.
    SearchLibrary,
    SearchNotes,
    /// On the keyboard: clear what's been typed.
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }
}

pub struct Screen {
    pub page: Page,
    pub hits: Vec<(Rect, Action)>,
    /// How many pages the list has.
    pub pages: usize,
    /// Words to mark grey, as a word looked up is: a hoarded word in its
    /// sentence.
    pub marks: Vec<Rect>,
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

/// A row of the type screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Face,
    Size,
    Measure,
    Leading,
    Setting,
}

/// The type screen as drawn by the library's fonts. The specimen, and each
/// face's name in that face, are drawn over it by the caller.
pub struct TypeScreen {
    pub screen: Screen,
    /// Where the specimen goes.
    pub specimen: Rect,
    /// Where each face's name goes: its left edge and baseline, and size.
    pub names: Vec<(f32, f32, f32)>,
}

/// A row in a list.
pub struct Row {
    pub title: String,
    /// The line under the title: author in small caps, series in italic.
    pub detail: Vec<Part>,
    /// A book's cover: its path in the library, and its size.
    pub cover: Option<(String, (u32, u32))>,
    /// How far through, for a book that's been opened.
    pub progress: Option<f32>,
    /// How many books, for an author, series or tag: set at the right,
    /// after leaders.
    pub count: Option<usize>,
    /// The chapter being read, in contents.
    pub mark: bool,
    pub action: Action,
    /// Indent steps, for contents.
    pub depth: usize,
}

/// A piece of a row's detail line, and how it's set.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Caps(String),
    Italic(String),
    Plain(String),
}

/// A book on the Continue shelf.
pub struct Recent {
    pub path: String,
    pub title: String,
    pub cover: Option<(u32, u32)>,
    pub progress: f32,
}

/// How a piece of text is set: its face and size, whether in small caps,
/// and the letterspacing, in ems. Figures are always old-style.
#[derive(Debug, Clone, Copy)]
struct Type {
    face: Face,
    size: f32,
    caps: bool,
    track: f32,
}

impl Type {
    fn new(face: Face, size: f32) -> Type {
        Type {
            face,
            size,
            caps: false,
            track: 0.0,
        }
    }

    /// Small caps, spaced by `track` ems. Capitals stay full height; set
    /// a label in lower case to have it all in small caps.
    fn caps(self, track: f32) -> Type {
        Type {
            caps: true,
            track,
            ..self
        }
    }
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

    /// A sign set large in Junicode, ending at `right` on `baseline`: what
    /// it is, and where.
    pub fn sign(&self, text: &str, pt: f32, right: f32, baseline: f32) -> (Page, Rect) {
        let mut page = Page::default();
        let t = Type::new(Face::Regular, self.pt(pt));
        let w = self.width(text, t);
        self.text(&mut page, text, t, right - w, baseline, w + 1.0);
        let pad = self.mm(1.5);
        let hit = Rect {
            x0: right - w - pad,
            y0: baseline - t.size - pad,
            x1: right + pad,
            y1: baseline + t.size * 0.3 + pad,
        };
        (page, hit)
    }

    /// The margin the library's screens keep, in pixels.
    pub fn margin_px(&self) -> f32 {
        self.margin()
    }

    /// Glyph runs for `text`, from x = 0, and their width.
    fn set(&self, text: &str, t: Type) -> (Vec<Run>, f32) {
        let features: &[&str] = if t.caps { &["onum", "smcp"] } else { &["onum"] };
        let track = t.track * t.size;
        let mut runs = Vec::new();
        let mut x = 0.0;
        let mut first = true;
        for s in self.fonts.shape(text, t.face, t.size, features) {
            let mut glyphs = Vec::with_capacity(s.glyphs.len());
            let mut extra = 0.0;
            for g in &s.glyphs {
                if !first {
                    extra += track;
                }
                first = false;
                glyphs.push((g.id, g.x + extra, g.y));
            }
            runs.push(Run {
                font: s.font,
                size: s.size,
                x,
                y: 0.0,
                glyphs,
            });
            x += s.width + extra;
        }
        (runs, x)
    }

    fn width(&self, text: &str, t: Type) -> f32 {
        self.set(text, t).1
    }

    /// Text at a baseline, cut with an ellipsis to fit `max` pixels. Returns
    /// its width.
    fn text(&self, page: &mut Page, text: &str, t: Type, x: f32, y: f32, max: f32) -> f32 {
        let mut set = self.set(text, t);
        if set.1 > max {
            let chars: Vec<char> = text.chars().collect();
            let mut n = chars.len();
            while n > 0 {
                n -= 1;
                let shown = chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
                set = self.set(&shown, t);
                if set.1 <= max {
                    break;
                }
            }
        }
        let (runs, width) = set;
        for mut r in runs {
            r.x += x;
            r.y = y;
            page.runs.push(r);
        }
        width
    }

    /// Text centred on the page.
    fn centred(&self, page: &mut Page, text: &str, t: Type, y: f32) -> f32 {
        let max = self.w - 2.0 * self.margin();
        let w = self.width(text, t).min(max);
        self.text(page, text, t, (self.w - w) / 2.0, y, max)
    }

    /// Dots from `x0` to `x1` on a baseline, on a grid shared by every
    /// row, so the leaders of a list line up.
    fn leaders(&self, page: &mut Page, x0: f32, x1: f32, y: f32, size: f32) {
        let Some(dot) = self
            .fonts
            .shape(".", Face::Regular, size, &[])
            .into_iter()
            .next()
        else {
            return;
        };
        let Some(id) = dot.glyphs.first().map(|g| g.id) else {
            return;
        };
        let step = size * 0.55;
        let mut x = (x0 / step).ceil() * step;
        let mut glyphs = Vec::new();
        while x + dot.width <= x1 {
            glyphs.push((id, x, 0.0));
            x += step;
        }
        page.runs.push(Run {
            font: dot.font,
            size: dot.size,
            x: 0.0,
            y,
            glyphs,
        });
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

    /// A hairline across the text block.
    fn hairline(&self, page: &mut Page, y: f32, grey: u8) {
        let m = self.margin();
        self.rule(page, m, y, self.w - 2.0 * m, 1.0, grey);
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
                let (dw, dh) = ((cw as f32 * scale).round(), (ch as f32 * scale).round());
                let (ix, iy) = ((x + (w - dw) / 2.0).round(), (y + (h - dh)).round());
                page.images.push(PageImage {
                    src: format!("{COVER}{path}"),
                    x: ix,
                    y: iy,
                    width: dw,
                    height: dh,
                });
                // A pale edge, for covers that are white at the edge.
                self.frame(page, ix - 1.0, iy - 1.0, dw + 2.0, dh + 2.0, 170);
            }
            _ => self.frame(page, x, y, w, h, 0),
        }
    }

    fn frame(&self, page: &mut Page, x: f32, y: f32, w: f32, h: f32, grey: u8) {
        self.rule(page, x, y, w, 1.0, grey);
        self.rule(page, x, y + h - 1.0, w, 1.0, grey);
        self.rule(page, x, y, 1.0, h, grey);
        self.rule(page, x + w - 1.0, y, 1.0, h, grey);
    }

    /// How far through: a hairline, with the part read drawn heavier.
    fn bar(&self, page: &mut Page, x: f32, y: f32, w: f32, share: f32) {
        self.rule(page, x, y, w, 1.0, 170);
        let t = self.mm(0.35).max(2.0);
        self.rule(page, x, y - t / 2.0 + 0.5, w * share.clamp(0.0, 1.0), t, 0);
    }

    /// Where what's under the header starts.
    pub fn header_bottom(&self) -> f32 {
        let size = self.pt(13.0);
        self.mm(9.0) + size + size * 0.9 + self.mm(3.0)
    }

    /// A page set elsewhere (a word's page in the hoard), under a header
    /// with ‹ to go back.
    pub fn titled(&self, title: &str, content: Page, page: usize, pages: usize) -> Screen {
        let mut s = Screen {
            page: content,
            hits: Vec::new(),
            marks: Vec::new(),
            pages,
        };
        self.header(&mut s, title, true, page);
        s
    }

    /// A gauge for the light, in a strip of paper down the left edge (how
    /// bright) or the right (how warm): its name, a hairline filled from
    /// the foot as far as `share`, and the figure. The page is in the
    /// strip's own coordinates; the rect is where the strip goes.
    pub fn gauge(&self, name: &str, figure: &str, share: f32, left: bool) -> (Page, Rect) {
        let mut page = Page::default();
        // Whole pixels, so the edge's hairline falls inside the strip.
        let w = self.mm(17.0).round();
        let (y0, y1) = ((self.h * 0.2).round(), (self.h * 0.8).round());
        let h = y1 - y0;
        let x = if left { 0.0 } else { self.w - w };
        // The strip's inner edge.
        self.rule(&mut page, if left { w - 1.0 } else { 0.0 }, 0.0, 1.0, h, 0);
        let label = Type::new(Face::Regular, self.pt(8.5)).caps(0.12);
        let lw = self.width(name, label);
        self.text(&mut page, name, label, (w - lw) / 2.0, self.mm(7.0), w);
        let t = Type::new(Face::Regular, self.pt(12.0));
        let fw = self.width(figure, t);
        self.text(&mut page, figure, t, (w - fw) / 2.0, h - self.mm(4.0), w);
        let (top, foot) = (self.mm(11.0), h - self.mm(11.0));
        let cx = (w / 2.0).round();
        self.rule(&mut page, cx, top, 1.0, foot - top, 170);
        let fill = (foot - top) * share.clamp(0.0, 1.0);
        let t = self.mm(0.6).max(3.0);
        self.rule(&mut page, cx - (t / 2.0).floor(), foot - fill, t, fill, 0);
        (
            page,
            Rect {
                x0: x,
                y0,
                x1: x + w,
                y1,
            },
        )
    }

    /// Where the typing screen's line of text is, top to bottom: what
    /// changes as a key is tapped.
    pub fn typing_band(&self) -> (f32, f32) {
        let top = self.header_bottom();
        (top, top + self.mm(24.0))
    }

    /// Typing: the text so far on a ruled line under the header, with a
    /// cursor; and the keyboard at the foot. Over the alphabet, a row of
    /// the old letters: þ ð æ ǫ ȝ ƿ œ ø. `shift`: the next letter is a
    /// capital.
    pub fn typing(&self, title: &str, text: &str, shift: bool) -> Screen {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            pages: 1,
            marks: Vec::new(),
        };
        self.header(&mut s, title, true, 0);
        let m = self.margin();
        let (_, band_bottom) = self.typing_band();
        let field = Type::new(Face::Regular, self.pt(18.0));
        let baseline = band_bottom - self.mm(8.0);
        // Clear, at the end of the line, once there's something to clear.
        let label = Type::new(Face::Regular, self.pt(10.0)).caps(0.14);
        let cw = self.width("clear", label);
        if !text.is_empty() {
            self.text(
                &mut s.page,
                "clear",
                label,
                self.w - m - cw,
                baseline,
                cw + 1.0,
            );
            s.hits.push((
                Rect {
                    x0: self.w - m - cw - self.mm(8.0),
                    y0: baseline - self.mm(12.0),
                    x1: self.w,
                    y1: baseline + self.mm(6.0),
                },
                Action::Clear,
            ));
        }
        let room = self.w - 2.0 * m - cw - self.mm(8.0);
        let tw = self.text(&mut s.page, text, field, m, baseline, room);
        let tw = if text.is_empty() { 0.0 } else { tw };
        self.rule(
            &mut s.page,
            (m + tw + self.mm(0.8)).round(),
            (baseline - field.size * 0.78).round(),
            2.0,
            (field.size * 0.98).round(),
            0,
        );
        self.hairline(&mut s.page, baseline + self.mm(3.0), 170);
        // The keys: four rows of letters and a row for space and search.
        let kh = self.mm(14.0);
        let side = self.mm(3.0);
        let kw = (self.w - 2.0 * side) / 10.0;
        let top = self.h - self.mm(5.0) - 5.0 * kh;
        let letter = Type::new(Face::Regular, self.pt(16.0));
        let label = Type::new(Face::Regular, self.pt(10.0)).caps(0.14);
        let up = |c: char| -> char {
            if shift {
                c.to_uppercase().next().unwrap_or(c)
            } else {
                c
            }
        };
        let rows = ["þðæǫȝƿœø", "qwertyuiop", "asdfghjkl", "zxcvbnm"];
        for (r, row) in rows.iter().enumerate() {
            let y = top + r as f32 * kh;
            self.rule(&mut s.page, side, y.round(), self.w - 2.0 * side, 1.0, 204);
            let n = row.chars().count() as f32;
            let left = (self.w - n * kw) / 2.0;
            let baseline = y + (kh + letter.size * 0.62) / 2.0;
            for (k, c) in row.chars().enumerate() {
                let c = up(c);
                let x = left + k as f32 * kw;
                let glyph = c.to_string();
                let gw = self.width(&glyph, letter);
                self.text(
                    &mut s.page,
                    &glyph,
                    letter,
                    x + (kw - gw) / 2.0,
                    baseline,
                    kw,
                );
                s.hits.push((
                    Rect {
                        x0: x,
                        y0: y,
                        x1: x + kw,
                        y1: y + kh,
                    },
                    Action::Key(c),
                ));
            }
            // Shift and delete flank the last row of letters.
            if r == 3 {
                let wide = left - side;
                let word_at = |s: &mut Screen, word: &str, x: f32, action: Action, on: bool| {
                    let ww = self.width(word, label);
                    let lx = x + (wide - ww) / 2.0;
                    let lb = y + (kh + label.size * 0.55) / 2.0;
                    self.text(&mut s.page, word, label, lx, lb, wide);
                    if on {
                        self.rule(&mut s.page, lx, (lb + self.mm(1.2)).round(), ww, 2.0, 0);
                    }
                    s.hits.push((
                        Rect {
                            x0: x,
                            y0: y,
                            x1: x + wide,
                            y1: y + kh,
                        },
                        action,
                    ));
                };
                word_at(&mut s, "shift", side, Action::Shift, shift);
                word_at(
                    &mut s,
                    "delete",
                    self.w - side - wide,
                    Action::Delete,
                    false,
                );
            }
        }
        // Space, and search, framed: what the typing is for.
        let y = top + 4.0 * kh;
        self.rule(&mut s.page, side, y.round(), self.w - 2.0 * side, 1.0, 204);
        let lb = y + (kh + label.size * 0.55) / 2.0;
        let (sx0, sx1) = (side + 1.5 * kw, side + 7.0 * kw);
        let sw = self.width("space", label);
        self.text(
            &mut s.page,
            "space",
            label,
            (sx0 + sx1 - sw) / 2.0,
            lb,
            sx1 - sx0,
        );
        s.hits.push((
            Rect {
                x0: sx0,
                y0: y,
                x1: sx1,
                y1: y + kh,
            },
            Action::Key(' '),
        ));
        let (dx0, dx1) = (side + 7.5 * kw, self.w - side - 0.5 * kw);
        let dw = self.width("search", label);
        self.frame(
            &mut s.page,
            dx0.round(),
            (y + kh * 0.18).round(),
            (dx1 - dx0).round(),
            (kh * 0.64).round(),
            0,
        );
        self.text(
            &mut s.page,
            "search",
            label,
            (dx0 + dx1 - dw) / 2.0,
            lb,
            dx1 - dx0,
        );
        s.hits.push((
            Rect {
                x0: dx0,
                y0: y,
                x1: self.w,
                y1: y + kh,
            },
            Action::Done,
        ));
        s
    }

    /// A few choices in a framed strip, as small caps between middle dots:
    /// above the rows from `above` to `below` if there's room, else under
    /// them. In the strip's own coordinates; then where the strip goes, and
    /// where each choice is on the screen.
    pub fn choices(&self, names: &[&str], above: f32, below: f32) -> (Page, Rect, Vec<Rect>) {
        let mut page = Page::default();
        let t = Type::new(Face::Regular, self.pt(10.0)).caps(0.14);
        let (pad, gap) = (self.mm(6.0), self.mm(9.0));
        let widths: Vec<f32> = names.iter().map(|n| self.width(n, t)).collect();
        let w = (widths.iter().sum::<f32>() + gap * (names.len() - 1) as f32 + 2.0 * pad).round();
        let h = self.mm(11.0).round();
        let top = if above - self.mm(3.0) - h >= 0.0 {
            above - self.mm(3.0) - h
        } else {
            below + self.mm(3.0)
        }
        .round();
        let left = ((self.w - w) / 2.0).round();
        self.frame(&mut page, 0.0, 0.0, w, h, 0);
        let baseline = (h + t.size * 0.55) / 2.0;
        let mut x = pad;
        let mut hits = Vec::new();
        for (k, name) in names.iter().enumerate() {
            if k > 0 {
                let dot = Type::new(Face::Regular, self.pt(10.0));
                let dw = self.width("·", dot);
                self.text(
                    &mut page,
                    "·",
                    dot,
                    x - gap / 2.0 - dw / 2.0,
                    baseline,
                    dw + 1.0,
                );
            }
            self.text(&mut page, name, t, x, baseline, widths[k] + 1.0);
            hits.push(Rect {
                x0: left + x - gap / 2.0,
                y0: top,
                x1: left + x + widths[k] + gap / 2.0,
                y1: top + h,
            });
            x += widths[k] + gap;
        }
        (
            page,
            Rect {
                x0: left,
                y0: top,
                x1: left + w,
                y1: top + h,
            },
            hits,
        )
    }

    /// A loupe: the word under a finger, set large in a framed box above
    /// it, so the finger doesn't hide it. In the box's own coordinates, and
    /// where the box goes: above (`x`, `y`) if there's room, else below.
    pub fn loupe(&self, word: &str, x: f32, y: f32) -> (Page, Rect) {
        let mut page = Page::default();
        let t = Type::new(Face::Regular, self.pt(22.0));
        let pad = self.mm(5.0);
        let w = (self.width(word, t) + 2.0 * pad)
            .max(self.mm(30.0))
            .min(self.w - 2.0 * self.mm(3.0))
            .round();
        let h = (t.size * 1.6 + pad).round();
        // Clear of the fingertip: well above it, or below near the top.
        let lift = self.mm(14.0);
        let top = if y - lift - h >= 0.0 {
            y - lift - h
        } else {
            y + lift
        };
        let left = (x - w / 2.0).clamp(self.mm(3.0), self.w - self.mm(3.0) - w);
        let (left, top) = (left.round(), top.round());
        self.frame(&mut page, 0.0, 0.0, w, h, 0);
        self.frame(&mut page, 1.0, 1.0, w - 2.0, h - 2.0, 0);
        let tw = self.width(word, t).min(w - 2.0 * pad);
        let baseline = (h + t.size * 0.62) / 2.0;
        self.text(&mut page, word, t, (w - tw) / 2.0, baseline, w - 2.0 * pad);
        (
            page,
            Rect {
                x0: left,
                y0: top,
                x1: left + w,
                y1: top + h,
            },
        )
    }

    /// Where the foot of a card begins: what's above it is the card's.
    pub fn card_foot(&self) -> f32 {
        self.h - self.mm(26.0)
    }

    /// A card of the hoard being unlocked: a page set elsewhere, under a
    /// header with ‹ to stop and how many are left. Before the entry is
    /// shown, a tap anywhere shows it; then the foot has the four grades,
    /// each with when the word would come back.
    pub fn card(&self, content: Page, left: usize, grades: Option<[String; 4]>) -> Screen {
        let mut s = Screen {
            page: content,
            hits: Vec::new(),
            marks: Vec::new(),
            pages: 1,
        };
        let top = self.header(&mut s, "unlocking", true, 0);
        let m = self.margin();
        let size = self.pt(13.0);
        let baseline = self.mm(9.0) + size;
        let t = Type::new(Face::Italic, self.pt(9.5));
        let label = format!("{left} to go");
        let lw = self.width(&label, t);
        self.text(&mut s.page, &label, t, self.w - m - lw, baseline, m * 3.0);
        let foot = self.card_foot();
        let caps = Type::new(Face::Regular, self.pt(11.0)).caps(0.14);
        match grades {
            None => {
                let y = foot + self.mm(13.0);
                self.centred(&mut s.page, "show the entry", caps, y);
                s.hits.push((
                    Rect {
                        x0: 0.0,
                        y0: top,
                        x1: self.w,
                        y1: self.h,
                    },
                    Action::Reveal,
                ));
            }
            Some(when) => {
                self.hairline(&mut s.page, foot + self.mm(3.0), 0);
                let col = (self.w - 2.0 * m) / 4.0;
                let small = Type::new(Face::Italic, self.pt(9.5));
                for (k, name) in crate::fsrs::GRADES.iter().enumerate() {
                    let x = m + k as f32 * col;
                    let y = foot + self.mm(12.0);
                    let nw = self.width(name, caps);
                    self.text(&mut s.page, name, caps, x + (col - nw) / 2.0, y, col);
                    let ww = self.width(&when[k], small).min(col);
                    self.text(
                        &mut s.page,
                        &when[k],
                        small,
                        x + (col - ww) / 2.0,
                        y + self.mm(6.0),
                        col,
                    );
                    s.hits.push((
                        Rect {
                            x0: x,
                            y0: foot,
                            x1: x + col,
                            y1: self.h,
                        },
                        Action::Grade(k as u8 + 1),
                    ));
                }
            }
        }
        s
    }

    /// A few lines in italic, centred on the page under a fleuron, with
    /// a header: the end of unlocking.
    pub fn centred_note(&self, title: &str, lines: &[String]) -> Screen {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            marks: Vec::new(),
            pages: 1,
        };
        self.header(&mut s, title, true, 0);
        let t = Type::new(Face::Italic, self.pt(14.0));
        let step = t.size * 1.6;
        let mut y = self.h * 0.42 - step * lines.len() as f32 / 2.0;
        self.centred(&mut s.page, "❧", Type::new(Face::Regular, self.pt(14.0)), y);
        y += self.mm(14.0);
        for line in lines {
            self.centred(&mut s.page, line, t, y);
            y += step;
        }
        s
    }

    /// The title along the top in spaced small caps, with ‹ to go back and
    /// the page count.
    fn header(&self, s: &mut Screen, title: &str, back: bool, page: usize) -> f32 {
        let m = self.margin();
        let size = self.pt(13.0);
        let baseline = self.mm(9.0) + size;
        let t = Type::new(Face::Regular, size).caps(0.12);
        let lower = title.to_lowercase();
        let tw = self.width(&lower, t).min(self.w - 6.0 * m);
        self.text(
            &mut s.page,
            &lower,
            t,
            (self.w - tw) / 2.0,
            baseline,
            self.w - 6.0 * m,
        );
        if back {
            self.text(
                &mut s.page,
                "‹",
                Type::new(Face::Regular, size * 1.5),
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
            let t = Type::new(Face::Italic, self.pt(9.5));
            let label = format!("{} of {}", page + 1, s.pages);
            let lw = self.width(&label, t);
            self.text(&mut s.page, &label, t, self.w - m - lw, baseline, m * 3.0);
        }
        let y = baseline + size * 0.9;
        self.hairline(&mut s.page, y, 0);
        y + self.mm(3.0)
    }

    /// Home: a masthead, the books read lately, then the shelves with
    /// their sizes.
    pub fn home(
        &self,
        recent: &[Recent],
        shelves: [(Shelf, usize); 4],
        hoard: usize,
        due: usize,
        underlines: usize,
    ) -> Screen {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            marks: Vec::new(),
            pages: 1,
        };
        let m = self.margin();
        // A short screen (the Libra Colour's) has a smaller masthead, without
        // its fleuron, so the shelves keep their room.
        let short = self.h * 25.4 / self.ppi < 170.0;
        let title = if short { 18.0 } else { 22.0 };
        let mut y = self.mm(if short { 8.0 } else { 11.0 }) + self.pt(title);
        self.centred(
            &mut s.page,
            "wordhord",
            Type::new(Face::Regular, self.pt(title)).caps(0.18),
            y,
        );
        y += self.pt(if short { 13.0 } else { 15.0 });
        self.centred(
            &mut s.page,
            "wordhord onleac",
            Type::new(Face::Italic, self.pt(10.5)),
            y,
        );
        if short {
            y += self.mm(9.0);
        } else {
            y += self.pt(20.0);
            self.centred(&mut s.page, "❧", Type::new(Face::Regular, self.pt(14.0)), y);
            y += self.mm(8.0);
        }
        let label = Type::new(Face::Regular, self.pt(10.0)).caps(0.14);
        if !recent.is_empty() {
            self.centred(&mut s.page, "continue reading", label, y);
            y += self.mm(4.0);
            let gap = self.mm(5.0);
            let small = Type::new(Face::Italic, self.pt(9.0));
            // The covers take what room the shelves leave them, each shelf
            // a comfortable row: on a small screen they're smaller, not
            // the shelves closer.
            let shelves_count = shelves.iter().filter(|s| s.1 > 0).count()
                + [hoard, due, underlines].iter().filter(|&&n| n > 0).count();
            let after = self.mm(if short { 7.0 } else { 12.0 });
            let below = self.mm(2.0) + small.size * 2.0 + after + self.mm(4.0);
            let shelves_h = shelves_count as f32 * self.mm(13.0);
            let close_top = self.h - self.mm(21.0);
            let room = close_top - self.mm(3.0) - shelves_h - below - y;
            let full = (self.w - 2.0 * m - 3.0 * gap) / 4.0;
            let ch = (full * 1.5).min(room).max(self.mm(18.0));
            let cw = ch / 1.5;
            // Fewer than four, or narrower, sit in the middle.
            let shown = recent.len().min(4);
            let span = shown as f32 * cw + (shown as f32 - 1.0) * gap;
            let left = (self.w - span) / 2.0;
            for (k, r) in recent.iter().take(4).enumerate() {
                let x = left + k as f32 * (cw + gap);
                self.cover(&mut s.page, &r.path, r.cover, x, y, cw, ch);
                self.bar(&mut s.page, x, y + ch + self.mm(2.0), cw, r.progress);
                self.text(
                    &mut s.page,
                    &r.title,
                    small,
                    x,
                    y + ch + self.mm(2.0) + small.size * 1.5,
                    cw,
                );
                s.hits.push((
                    Rect {
                        x0: x,
                        y0: y,
                        x1: x + cw,
                        y1: y + ch + self.mm(2.0) + small.size * 2.0,
                    },
                    Action::Open(r.path.clone()),
                ));
            }
            y += ch + self.mm(2.0) + small.size * 2.0 + after;
        }
        self.centred(&mut s.page, "the shelves", label, y);
        // Search, at the right of the shelves' heading: what it searches.
        let sw = self.width("search", label);
        self.text(&mut s.page, "search", label, self.w - m - sw, y, sw + 1.0);
        s.hits.push((
            Rect {
                x0: self.w - m - sw - self.mm(10.0),
                y0: y - self.mm(8.0),
                x1: self.w,
                y1: y + self.mm(3.0),
            },
            Action::SearchLibrary,
        ));
        y += self.mm(4.0);
        let name = Type::new(Face::Regular, self.pt(14.0));
        let figure = Type::new(Face::Regular, self.pt(12.0));
        // The shelves, and the hoard of words kept, once there is one.
        let rows: Vec<_> = shelves
            .iter()
            .map(|(shelf, count)| (shelf.name(), Some(*count), Action::Shelf(*shelf)))
            .chain((hoard > 0).then_some(("The hoard", Some(hoard), Action::Hoard)))
            .chain((due > 0).then_some(("Unlock the hoard", Some(due), Action::Unlock)))
            .chain((underlines > 0).then_some((
                "Commonplace book",
                Some(underlines),
                Action::Commonplace,
            )))
            .filter(|r| r.1 > Some(0))
            .collect();
        // Close, at the foot, keeps its own ground: the rows close up to
        // stay clear of it.
        let foot = self.h - self.mm(12.0);
        let close_top = foot - self.mm(9.0);
        let row = self
            .mm(14.0)
            .min((close_top - self.mm(3.0) - y) / rows.len().max(1) as f32);
        for (title, count, action) in rows {
            let baseline = y + (row + name.size * 0.6) / 2.0;
            let nw = self.text(&mut s.page, title, name, m, baseline, self.w);
            if let Some(count) = count {
                let n = count.to_string();
                let fw = self.width(&n, figure);
                self.text(&mut s.page, &n, figure, self.w - m - fw, baseline, self.w);
                self.leaders(
                    &mut s.page,
                    m + nw + self.mm(3.0),
                    self.w - m - fw - self.mm(3.0),
                    baseline,
                    figure.size,
                );
            }
            s.hits.push((
                Rect {
                    x0: 0.0,
                    y0: y,
                    x1: self.w,
                    y1: y + row,
                },
                action,
            ));
            y += row;
        }
        let close = "close wordhord";
        let w = self.centred(&mut s.page, close, label, foot);
        s.hits.push((
            Rect {
                x0: (self.w - w) / 2.0 - self.mm(10.0),
                y0: close_top,
                x1: (self.w + w) / 2.0 + self.mm(10.0),
                y1: self.h,
            },
            Action::Close,
        ));
        s
    }

    /// A list: a page of its rows, under a title. Books are set like a
    /// catalogue: title in italic, author in small caps, then the series.
    /// Authors, series and tags run to their counts on leaders. Contents
    /// set parts in spaced small caps and mark the chapter being read.
    pub fn list(&self, title: &str, rows: &[Row], page: usize, back: bool) -> Screen {
        let books = rows.iter().any(|r| r.cover.is_some());
        let contents = rows
            .iter()
            .all(|r| r.count.is_none() && r.cover.is_none() && r.detail.is_empty());
        let row_h = if books {
            self.mm(22.0)
        } else if contents {
            self.mm(9.5)
        } else {
            self.mm(12.0)
        };
        let top_guess = self.mm(9.0) + self.pt(13.0) * 1.9 + self.mm(3.0);
        let per_page = (((self.h - top_guess - self.mm(8.0)) / row_h).floor() as usize).max(1);
        let pages = rows.len().div_ceil(per_page).max(1);
        let page = page.min(pages - 1);
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            marks: Vec::new(),
            pages,
        };
        let m = self.margin();
        let mut y = self.header(&mut s, title, back, page);
        let size = self.pt(11.5);
        let small = self.pt(9.0);
        let first_row = page * per_page;
        for (k, r) in rows.iter().enumerate().skip(first_row).take(per_page) {
            let mut x = m + r.depth as f32 * self.mm(6.0);
            if books {
                let (cw, ch) = (self.mm(12.0), self.mm(18.0));
                if let Some((path, size)) = &r.cover {
                    let size = Some(*size).filter(|s| s.0 > 0);
                    self.cover(&mut s.page, path, size, x, y + self.mm(2.0), cw, ch);
                }
                x += cw + self.mm(5.0);
            }
            // A part of the contents: an entry with entries under it.
            let part = contents && rows.get(k + 1).is_some_and(|n| n.depth > r.depth);
            let lines = if r.detail.is_empty() { 1.0 } else { 2.0 };
            let first = if books {
                y + self.mm(2.0) + size
            } else {
                y + row_h / 2.0 + size * 0.3 - (lines - 1.0) * small * 0.7
            };
            let mut tt = if books || r.detail.iter().any(|p| matches!(p, Part::Caps(_))) {
                Type::new(Face::Italic, size)
            } else {
                Type::new(Face::Regular, size)
            };
            let mut text = r.title.clone();
            if part {
                tt = Type::new(Face::Regular, size).caps(0.1);
                text = text.to_lowercase();
            }
            let right = match r.count {
                Some(n) => {
                    let t = Type::new(Face::Regular, size);
                    let n = n.to_string();
                    let w = self.width(&n, t);
                    self.text(&mut s.page, &n, t, self.w - m - w, first, w + 1.0);
                    w + self.mm(3.0)
                }
                None => 0.0,
            };
            let width = self.w - m - x - right;
            let tw = self.text(&mut s.page, &text, tt, x, first, width);
            if r.count.is_some() {
                self.leaders(
                    &mut s.page,
                    x + tw + self.mm(3.0),
                    self.w - m - right,
                    first,
                    size,
                );
            }
            if r.mark {
                let t = Type::new(Face::Regular, size * 0.8);
                let w = self.width("❧", t);
                self.text(&mut s.page, "❧", t, x - w - self.mm(2.5), first, w + 1.0);
            }
            if !r.detail.is_empty() {
                let mut dx = x;
                let dy = first + small * 1.6;
                for p in &r.detail {
                    let (t, text) = match p {
                        Part::Caps(t) => (Type::new(Face::Regular, small).caps(0.04), t),
                        Part::Italic(t) => (Type::new(Face::Italic, small), t),
                        Part::Plain(t) => (Type::new(Face::Regular, small), t),
                    };
                    let left = self.w - m - right - dx;
                    if left < small {
                        break;
                    }
                    dx += self.text(&mut s.page, text, t, dx, dy, left);
                }
            }
            if let Some(p) = r.progress {
                self.bar(&mut s.page, x, first + small * 3.3, self.mm(30.0), p);
            }
            if !contents {
                self.hairline(&mut s.page, y + row_h, 204);
            }
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

    /// The type screen: a specimen passage, set as chosen; then the
    /// faces, each marked with the old languages it can set; then size,
    /// measure, leading and setting, each a row of names to tap, the one
    /// chosen in italic. `chosen` is each row's choice, in `Choice` order.
    pub fn type_screen(
        &self,
        rows: &[(Choice, &str, Vec<&str>)],
        chosen: &[usize],
        faces: &[Vec<&str>],
        note: Option<&str>,
    ) -> TypeScreen {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            marks: Vec::new(),
            pages: 1,
        };
        let top = self.header(&mut s, "Type", true, 0);
        // The choices first, measured, closer set on a small screen; the
        // specimen takes the height they leave, two to five centimetres.
        let fits = |tight: bool| {
            let mut scratch = Screen {
                page: Page::default(),
                hits: Vec::new(),
                marks: Vec::new(),
                pages: 1,
            };
            let mut names = Vec::new();
            let end = self.type_choices(&mut scratch, &mut names, 0.0, rows, chosen, faces, tight);
            self.h - self.mm(4.0) - top - end
        };
        let tight = fits(false) < self.mm(30.0);
        let room = fits(tight);
        let specimen_h = room.clamp(self.mm(20.0), self.mm(50.0));
        let specimen = Rect {
            x0: 0.0,
            y0: top,
            x1: self.w,
            y1: top + specimen_h,
        };
        let y = specimen.y1;
        self.hairline(&mut s.page, y, 0);
        if let Some(note) = note {
            let t = Type::new(Face::Italic, self.pt(9.0));
            self.centred(&mut s.page, note, t, y + self.mm(2.0) + t.size);
        }
        let mut names = Vec::new();
        self.type_choices(&mut s, &mut names, y, rows, chosen, faces, tight);
        TypeScreen {
            screen: s,
            specimen,
            names,
        }
    }

    /// The type screen's choices, from `y`, where the specimen ends: the
    /// faces, each with the old languages it can set, then a row of names
    /// for each other choice, wrapping as the screen needs. `tight` sets
    /// them closer. Where they end.
    #[allow(clippy::too_many_arguments)]
    fn type_choices(
        &self,
        s: &mut Screen,
        names: &mut Vec<(f32, f32, f32)>,
        mut y: f32,
        rows: &[(Choice, &str, Vec<&str>)],
        chosen: &[usize],
        faces: &[Vec<&str>],
        tight: bool,
    ) -> f32 {
        let m = self.margin();
        let k = if tight { 0.8 } else { 1.0 };
        y += self.mm(9.0 * k);
        let label = Type::new(Face::Regular, self.pt(9.0)).caps(0.14);
        let face_choice = chosen.first().copied().unwrap_or(0);
        self.text(&mut s.page, "face", label, m, y, self.w);
        y += self.mm(2.0);
        let row = self.mm(9.0 * k);
        let small = Type::new(Face::Regular, self.pt(8.5)).caps(0.06);
        for (n, languages) in faces.iter().enumerate() {
            let baseline = y + row * 0.68;
            names.push((m, baseline, self.pt(13.0)));
            let list = languages.join(" · ");
            let lw = self.width(&list, small);
            self.text(
                &mut s.page,
                &list,
                small,
                self.w - m - lw,
                baseline,
                lw + 1.0,
            );
            if n == face_choice {
                let t = Type::new(Face::Regular, self.pt(10.5));
                let w = self.width("❧", t);
                self.text(&mut s.page, "❧", t, m - w - self.mm(2.5), baseline, w + 1.0);
            }
            s.hits.push((
                Rect {
                    x0: 0.0,
                    y0: y,
                    x1: self.w,
                    y1: y + row,
                },
                Action::Choose(Choice::Face, n),
            ));
            y += row;
        }
        y += self.mm(5.0 * k);
        let word = Type::new(Face::Regular, self.pt(12.0));
        let sep = "  ·  ";
        let sep_w = self.width(sep, word);
        for (n, (choice, name, options)) in rows.iter().enumerate() {
            self.text(&mut s.page, name, label, m, y, self.w);
            let mut baseline = y + self.mm(7.5 * k);
            let mut x = m;
            let now = chosen.get(n + 1).copied().unwrap_or(0);
            for (o, option) in options.iter().enumerate() {
                let t = if o == now {
                    Type::new(Face::Italic, word.size)
                } else {
                    word
                };
                let w = self.width(option, t);
                if o > 0 {
                    // Too wide for the line: the next one.
                    if x + sep_w + w > self.w - m {
                        baseline += self.mm(7.0 * k);
                        x = m;
                    } else {
                        x += self.text(&mut s.page, sep, word, x, baseline, self.w);
                    }
                }
                self.text(&mut s.page, option, t, x, baseline, self.w);
                if o == now {
                    self.rule(&mut s.page, x, baseline + word.size * 0.25, w, 1.0, 0);
                }
                // Generous to tap: up to halfway to the next word.
                s.hits.push((
                    Rect {
                        x0: x - self.mm(2.0),
                        y0: baseline - word.size - self.mm(1.5),
                        x1: x + w + self.mm(2.0),
                        y1: baseline + self.mm(3.0),
                    },
                    Action::Choose(*choice, o),
                ));
                x += w;
            }
            y = baseline + self.mm(9.0 * k);
        }
        y
    }

    /// A label in spaced small caps, or italic, ending at `right` on a
    /// baseline, as a page to draw over another, and the place to tap it.
    pub fn label(&self, text: &str, italic: bool, right: f32, baseline: f32) -> (Page, Rect) {
        let mut page = Page::default();
        let size = self.pt(8.5);
        let t = if italic {
            Type::new(Face::Italic, self.pt(9.5))
        } else {
            Type::new(Face::Regular, size).caps(0.14)
        };
        let w = self.width(text, t);
        self.text(&mut page, text, t, right - w, baseline, w + 1.0);
        let pad = self.mm(4.0);
        let hit = Rect {
            x0: right - w - pad,
            y0: baseline - size - pad,
            x1: right + pad,
            y1: baseline + pad,
        };
        (page, hit)
    }

    /// The bar over the top of a page while reading, in two lines: the way
    /// back to the library, the type and the contents along the top, in
    /// small caps; the book's title under them, in italic, across the whole
    /// width, so even a narrow screen has room for it.
    pub fn reading_bar(&self, title: &str) -> (Screen, u32) {
        let mut s = Screen {
            page: Page::default(),
            hits: Vec::new(),
            marks: Vec::new(),
            pages: 1,
        };
        let m = self.margin();
        let size = self.pt(11.0);
        let controls = self.mm(13.0);
        let height = (controls + self.mm(11.0)).round();
        let baseline = (controls + size * 0.6) / 2.0;
        let label = Type::new(Face::Regular, size).caps(0.12);
        let lw = self.text(&mut s.page, "‹ library", label, m, baseline, self.w / 3.0);
        let cw = self.width("contents", label);
        self.text(
            &mut s.page,
            "contents",
            label,
            self.w - m - cw,
            baseline,
            cw + 1.0,
        );
        let yw = self.width("type", label);
        let yx = self.w - m - cw - self.mm(8.0) - yw;
        self.text(&mut s.page, "type", label, yx, baseline, yw + 1.0);
        s.hits.push((
            Rect {
                x0: 0.0,
                y0: 0.0,
                x1: m + lw + m,
                y1: controls,
            },
            Action::Library,
        ));
        s.hits.push((
            Rect {
                x0: yx - self.mm(4.0),
                y0: 0.0,
                x1: yx + yw + self.mm(4.0),
                y1: controls,
            },
            Action::Type,
        ));
        s.hits.push((
            Rect {
                x0: self.w - m - cw - self.mm(4.0),
                y0: 0.0,
                x1: self.w,
                y1: controls,
            },
            Action::Contents,
        ));
        // A hairline between, then the title, centred.
        self.hairline(&mut s.page, controls.round(), 204);
        let t = Type::new(Face::Italic, self.pt(11.5));
        let room = self.w - 2.0 * m;
        let tw = self.width(title, t).min(room);
        self.text(
            &mut s.page,
            title,
            t,
            (self.w - tw) / 2.0,
            controls + (height - controls + t.size * 0.6) / 2.0,
            room,
        );
        self.rule(&mut s.page, 0.0, height - 2.0, self.w, 2.0, 0);
        (s, height as u32)
    }
}
