//! What the reader shows, and what each gesture does to it. There's no
//! screen here: the device draws what this decides.
//!
//! - A swipe turns the page. So does a tap near the left or right edge,
//!   unless it lands on something tappable there, like a stanza number.
//! - A tap on a note marker, a commentary mark or a stanza number opens it
//!   in the panel over the foot of the page.
//! - With the panel open, a tap in it turns its pages (right half on, left
//!   half back) or follows a link in it. A tap on another link outside it
//!   opens that instead. A tap anywhere else closes it.
//! - A hold, or the power button, closes the reader.

use std::collections::HashMap;

use wordhord_typeset::page::{Link, Target, Typeset};

use crate::touch::Gesture;

/// A full refresh every so many pages clears the ghosts of earlier ones.
const FLASH_EVERY: usize = 6;
/// Taps this close to the left or right edge, as a share of the width,
/// turn the page.
const EDGE: f32 = 0.15;
/// Taps this close to the top, as a share of the height, bring up the bar
/// with the way back to the library and the contents.
const TOP: f32 = 0.08;
/// A tap that misses every link, but by less than this many pixels, takes
/// the nearest: fingers are blunt, and links sit close to gaps.
const NEAR: f32 = 40.0;

/// A note open in the panel: which, and which of its pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open {
    pub note: usize,
    pub page: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Nothing,
    /// Draw the current page; flash to clear ghosting.
    Page {
        flash: bool,
    },
    /// Draw the panel's current page over the foot of the screen.
    Panel,
    /// Put the page back where the panel was.
    ClosePanel,
    /// Past the chapter's end (1) or before its start (-1): the next or
    /// previous chapter.
    Chapter(i8),
    /// Bring up the bar over the top of the page.
    Menu,
    /// Look up a word: its place among the page's words.
    Look(usize),
    Close,
}

pub struct Reader<'t> {
    t: &'t Typeset,
    pub page: usize,
    pub open: Option<Open>,
    turns: usize,
    ids: HashMap<&'t str, usize>,
}

impl<'t> Reader<'t> {
    pub fn new(t: &'t Typeset, page: usize) -> Reader<'t> {
        Reader {
            t,
            page: page.min(t.pages.len().saturating_sub(1)),
            open: None,
            turns: 0,
            ids: t
                .notes
                .iter()
                .enumerate()
                .map(|(k, n)| (n.id.as_str(), k))
                .collect(),
        }
    }

    pub fn act(&mut self, g: Gesture) -> Effect {
        match g {
            // Held on a word, anywhere (by the edges too, where a tap
            // turns the page): look it up.
            Gesture::Hold(x, y) => word_at(&self.t.pages[self.page], x as f32, y as f32)
                .map_or(Effect::Nothing, Effect::Look),
            Gesture::Power => Effect::Close,
            Gesture::SwipeLeft => self.turn(1),
            Gesture::SwipeRight => self.turn(-1),
            Gesture::Tap(x, y) => self.tap(x as f32, y as f32),
        }
    }

    fn turn(&mut self, by: isize) -> Effect {
        let to = self.page as isize + by;
        if to < 0 || to as usize >= self.t.pages.len() {
            return Effect::Chapter(by.signum() as i8);
        }
        self.page = to as usize;
        self.open = None;
        self.turns += 1;
        Effect::Page {
            flash: self.turns.is_multiple_of(FLASH_EVERY),
        }
    }

    fn tap(&mut self, x: f32, y: f32) -> Effect {
        let t = self.t;
        let p = &t.panel;
        if let Some(open) = self.open {
            if y >= p.y {
                let (px, py) = (x - p.x, y - p.y);
                let note = &t.notes[open.note];
                if let Some(l) = link_at(&note.pages[open.page].links, px, py) {
                    return self.follow(&l.to);
                }
                let page = if px >= p.width / 2.0 {
                    open.page + 1
                } else {
                    open.page.wrapping_sub(1)
                };
                if page < note.pages.len() {
                    self.open = Some(Open { page, ..open });
                    return Effect::Panel;
                }
                return Effect::Nothing;
            }
            if let Some(l) = link_at(&t.pages[self.page].links, x, y) {
                return self.follow(&l.to);
            }
            self.open = None;
            return Effect::ClosePanel;
        }
        if let Some(l) = link_at(&t.pages[self.page].links, x, y) {
            return self.follow(&l.to);
        }
        if y < t.height as f32 * TOP {
            return Effect::Menu;
        }
        let width = t.width as f32;
        if x < width * EDGE {
            self.turn(-1)
        } else if x > width * (1.0 - EDGE) {
            self.turn(1)
        } else {
            word_at(&t.pages[self.page], x, y).map_or(Effect::Nothing, Effect::Look)
        }
    }

    fn follow(&mut self, to: &Target) -> Effect {
        match to {
            Target::Note { id } => match self.ids.get(id.as_str()) {
                Some(&note) if !self.t.notes[note].pages.is_empty() => {
                    self.open = Some(Open { note, page: 0 });
                    Effect::Panel
                }
                _ => Effect::Nothing,
            },
            Target::Page { page } if *page < self.t.pages.len() => {
                self.page = *page;
                self.open = None;
                Effect::Page { flash: false }
            }
            _ => Effect::Nothing,
        }
    }
}

/// The word under a point, or the nearest on its line if the tap fell
/// between two.
fn word_at(page: &wordhord_typeset::page::Page, x: f32, y: f32) -> Option<usize> {
    let on_line = |w: &&wordhord_typeset::page::Word| (w.y0..=w.y1).contains(&y);
    page.words
        .iter()
        .enumerate()
        .filter(|(_, w)| on_line(w))
        .min_by(|(_, a), (_, b)| {
            let d = |w: &wordhord_typeset::page::Word| (w.x0 - x).max(x - w.x1).max(0.0);
            d(a).total_cmp(&d(b))
        })
        .filter(|(_, w)| (w.x0 - x).max(x - w.x1) < 30.0)
        .map(|(k, _)| k)
}

/// The link under a point: of those whose boxes hold it, the one whose
/// middle is nearest. Failing that, the nearest box within `NEAR`. A link
/// below the point counts as nearer than one as far above it: a stanza's
/// number is at its top, so a tap in the space between two stanzas means
/// the one below.
pub fn link_at(links: &[Link], x: f32, y: f32) -> Option<&Link> {
    let middle = |l: &Link| ((l.x0 + l.x1) / 2.0 - x).powi(2) + ((l.y0 + l.y1) / 2.0 - y).powi(2);
    let gap = |l: &Link| {
        let dx = (l.x0 - x).max(x - l.x1).max(0.0);
        let dy = if l.y0 > y {
            (l.y0 - y) / 2.0
        } else {
            (y - l.y1).max(0.0)
        };
        (dx * dx + dy * dy).sqrt()
    };
    links
        .iter()
        .filter(|l| (l.x0..=l.x1).contains(&x) && (l.y0..=l.y1).contains(&y))
        .min_by(|a, b| middle(a).total_cmp(&middle(b)))
        .or_else(|| {
            links
                .iter()
                .filter(|l| gap(l) <= NEAR)
                .min_by(|a, b| gap(a).total_cmp(&gap(b)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordhord_typeset::page::{Note, Page, Panel};

    fn link(to: Target, x0: f32, y0: f32, x1: f32, y1: f32) -> Link {
        Link { to, x0, y0, x1, y1 }
    }

    fn note(id: &str) -> Target {
        Target::Note { id: id.into() }
    }

    /// Three pages. Page 0 has a marker for a two-page note at (500, 100),
    /// and a stanza number in the left margin at (60, 300).
    fn book() -> Typeset {
        let mut first = Page::default();
        first
            .links
            .push(link(note("note-1"), 470.0, 80.0, 530.0, 120.0));
        first
            .links
            .push(link(note("vsp.1"), 40.0, 280.0, 100.0, 320.0));
        let panel_page = |links| Page {
            links,
            ..Default::default()
        };
        Typeset {
            profile: "test".into(),
            width: 1404,
            height: 1872,
            fonts: vec![],
            pages: vec![first, Page::default(), Page::default()],
            panel: Panel {
                x: 0.0,
                y: 1123.0,
                width: 1404.0,
                height: 749.0,
            },
            notes: vec![
                Note {
                    id: "note-1".into(),
                    kind: "note".into(),
                    label: "1".into(),
                    title: "1".into(),
                    on: None,
                    page: Some(0),
                    pages: vec![panel_page(vec![]), panel_page(vec![])],
                },
                Note {
                    id: "vsp.1".into(),
                    kind: "translation".into(),
                    label: "vsp.1".into(),
                    title: "Stanza 1".into(),
                    on: None,
                    page: Some(0),
                    pages: vec![panel_page(vec![])],
                },
            ],
        }
    }

    #[test]
    fn swipes_and_edge_taps_turn_pages() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::SwipeRight), Effect::Chapter(-1));
        assert_eq!(r.act(Gesture::SwipeLeft), Effect::Page { flash: false });
        assert_eq!(
            r.act(Gesture::Tap(1380, 900)),
            Effect::Page { flash: false }
        );
        assert_eq!(r.page, 2);
        assert_eq!(r.act(Gesture::Tap(1380, 900)), Effect::Chapter(1));
        assert_eq!(r.act(Gesture::Tap(20, 900)), Effect::Page { flash: false });
        assert_eq!(r.page, 1);
        // The middle of the page does nothing.
        assert_eq!(r.act(Gesture::Tap(700, 900)), Effect::Nothing);
    }

    #[test]
    fn a_link_in_the_margin_wins_over_turning_the_page() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::Tap(60, 300)), Effect::Panel);
        assert_eq!(r.open, Some(Open { note: 1, page: 0 }));
        assert_eq!(r.page, 0);
    }

    #[test]
    fn the_panel_pages_switches_and_closes() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::Tap(500, 100)), Effect::Panel);
        // Right half of the panel: its next page; there's no third.
        assert_eq!(r.act(Gesture::Tap(1000, 1500)), Effect::Panel);
        assert_eq!(r.open, Some(Open { note: 0, page: 1 }));
        assert_eq!(r.act(Gesture::Tap(1000, 1500)), Effect::Nothing);
        assert_eq!(r.act(Gesture::Tap(300, 1500)), Effect::Panel);
        assert_eq!(r.open, Some(Open { note: 0, page: 0 }));
        // Another link above the panel switches it.
        assert_eq!(r.act(Gesture::Tap(60, 300)), Effect::Panel);
        assert_eq!(r.open, Some(Open { note: 1, page: 0 }));
        // Anywhere else above it closes it, even at the edge.
        assert_eq!(r.act(Gesture::Tap(1380, 600)), Effect::ClosePanel);
        assert_eq!((r.open, r.page), (None, 0));
    }

    #[test]
    fn turning_the_page_closes_the_panel() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        r.act(Gesture::Tap(500, 100));
        assert_eq!(r.act(Gesture::SwipeLeft), Effect::Page { flash: false });
        assert_eq!(r.open, None);
    }

    #[test]
    fn every_sixth_turn_flashes() {
        let t = Typeset {
            pages: vec![Page::default(); 10],
            ..book()
        };
        let mut r = Reader::new(&t, 0);
        let flashes: Vec<bool> = (0..7)
            .map(|_| r.act(Gesture::SwipeLeft) == Effect::Page { flash: true })
            .collect();
        assert_eq!(flashes, [false, false, false, false, false, true, false]);
    }

    #[test]
    fn a_tap_just_off_a_link_still_takes_it() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        // 25 px above the stanza number's box.
        assert_eq!(r.act(Gesture::Tap(60, 255)), Effect::Panel);
        assert_eq!(r.open, Some(Open { note: 1, page: 0 }));
    }

    #[test]
    fn a_tap_at_the_top_brings_up_the_bar() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::Tap(700, 60)), Effect::Menu);
        // Even near an edge: the top wins over turning the page.
        assert_eq!(r.act(Gesture::Tap(1380, 60)), Effect::Menu);
        assert_eq!(r.page, 0);
    }

    #[test]
    fn a_tap_on_a_word_looks_it_up() {
        let mut t = book();
        t.pages[0].words.push(wordhord_typeset::page::Word {
            text: "mǫgu".into(),
            continues: false,
            block: "b1".into(),
            x0: 600.0,
            y0: 880.0,
            x1: 700.0,
            y1: 920.0,
            letters: None,
        });
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::Tap(650, 900)), Effect::Look(0));
        // Just past the word's end still means it.
        assert_eq!(r.act(Gesture::Tap(720, 900)), Effect::Look(0));
        // Blank space means nothing.
        assert_eq!(r.act(Gesture::Tap(650, 1300)), Effect::Nothing);
    }

    #[test]
    fn a_word_by_the_edge_is_held_to_look_it_up() {
        let mut t = book();
        t.pages[0].words.push(wordhord_typeset::page::Word {
            text: "hwæt".into(),
            continues: false,
            block: "b1".into(),
            x0: 1250.0,
            y0: 880.0,
            x1: 1330.0,
            y1: 920.0,
            letters: None,
        });
        let mut r = Reader::new(&t, 0);
        assert_eq!(r.act(Gesture::Hold(1300, 900)), Effect::Look(0));
        assert_eq!(r.page, 0);
        // A tap there still turns the page.
        assert!(matches!(
            r.act(Gesture::Tap(1300, 900)),
            Effect::Page { .. }
        ));
    }

    #[test]
    fn power_closes_and_a_hold_off_a_word_does_nothing() {
        let t = book();
        let mut r = Reader::new(&t, 0);
        // Held off a word, nothing: closing is from the library.
        assert_eq!(r.act(Gesture::Hold(650, 1300)), Effect::Nothing);
        assert_eq!(r.act(Gesture::Power), Effect::Close);
    }
}
