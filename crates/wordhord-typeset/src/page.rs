//! The compiled form (SPEC.md §7): pages of glyph runs, positioned by the
//! desktop, and a map of every word on them. The device only draws.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Typeset {
    pub profile: String,
    pub width: u32,
    pub height: u32,
    /// The fonts glyph IDs refer to, pinned by hash.
    pub fonts: Vec<FontRef>,
    pub pages: Vec<Page>,
    /// Where a note opens: a panel over the foot of the page.
    pub panel: Panel,
    /// Every note, in the order its marker appears.
    pub notes: Vec<Note>,
}

/// The note panel's box on the screen.
#[derive(Debug, Clone, Serialize)]
pub struct Panel {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A note or a commentary, set for the panel. Its pages are drawn from
/// the panel's top-left corner.
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    /// What links to it: `note-3`, or the commentary's block ID.
    pub id: String,
    /// `note` or `commentary`.
    pub kind: String,
    /// The note's label, or the reference the commentary is on.
    pub label: String,
    /// What a reader sees: the label, `Stanza 5`, or a passage's first
    /// words.
    pub title: String,
    /// The reference of the line its marker is on: `inf.1.2`.
    pub on: Option<String>,
    /// The page its marker is on, from 0.
    pub page: Option<usize>,
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FontRef {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Page {
    pub runs: Vec<Run>,
    pub words: Vec<Word>,
    pub links: Vec<Link>,
}

/// A box to tap, and where it goes.
#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub to: Target,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// Open a note or a commentary in the panel.
    Note { id: String },
    /// Turn to a page, counted from 0.
    Page { page: usize },
    /// Back to a note's marker or a commentary's mark: resolved to a page once the chapter is
    /// paginated, and never written.
    #[serde(skip)]
    Marker(String),
}

/// Glyphs in one font and size, from a baseline origin.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub font: usize,
    pub size: f32,
    pub x: f32,
    pub y: f32,
    /// Glyph ID, and its offset from the run's origin.
    pub glyphs: Vec<(u16, f32, f32)>,
}

/// A word on the page: its text, its box, and the block it's in.
#[derive(Debug, Clone, Serialize)]
pub struct Word {
    pub text: String,
    /// This is the rest of the word before it, hyphenated across lines.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub continues: bool,
    pub block: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}
