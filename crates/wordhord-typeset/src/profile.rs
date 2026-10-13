//! A device profile (`profiles/*.toml`): the screen, the margins, the type
//! size and the fonts.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub ppi: f32,
    /// In millimetres.
    pub margins: Margins,
    /// In points.
    pub body_size: f32,
    /// Baseline to baseline, as a multiple of the size.
    pub leading: f32,
    /// Prose justified, or set ragged right.
    #[serde(default = "justified")]
    pub justify: bool,
    pub fonts: FontPaths,
    /// The folder the profile was read from: font paths are relative to it.
    #[serde(skip)]
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Margins {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontPaths {
    pub regular: String,
    pub italic: String,
    pub bold: String,
    pub bold_italic: String,
    /// For code.
    pub mono: Option<String>,
    pub fallback: Option<String>,
    /// After the fallback, fonts for scripts it lacks (Hebrew, Arabic,
    /// Devanagari), tried in order.
    #[serde(default)]
    pub fallbacks: Vec<String>,
}

fn justified() -> bool {
    true
}

impl Profile {
    pub fn load(path: &Path) -> Result<Profile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut p: Profile =
            toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
        p.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok(p)
    }

    /// Millimetres to pixels.
    pub fn mm(&self, mm: f32) -> f32 {
        mm * self.ppi / 25.4
    }

    /// Points to pixels.
    pub fn pt(&self, pt: f32) -> f32 {
        pt * self.ppi / 72.0
    }
}
