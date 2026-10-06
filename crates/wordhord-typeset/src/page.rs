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
    pub block: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}
