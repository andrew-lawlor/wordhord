//! Reading settings: the face, its size, the measure, the leading and how
//! prose is set. Kept in `wordhord.toml` in the app folder, by name, so the
//! file reads plainly and can be edited by hand. A name Wordhord doesn't
//! know reads as the default.

use std::path::Path;

use serde::{Deserialize, Serialize};
use wordhord_typeset::fonts::Fonts;
use wordhord_typeset::profile::Profile;

/// A face Wordhord ships: its name, and its files in the app's `fonts`
/// folder: regular, italic, bold, bold italic.
pub struct Typeface {
    pub key: &'static str,
    pub name: &'static str,
    pub files: [&'static str; 4],
}

pub const FACES: [Typeface; 5] = [
    Typeface {
        key: "junicode",
        name: "Junicode",
        files: [
            "Junicode-Regular.otf",
            "Junicode-Italic.otf",
            "Junicode-Bold.otf",
            "Junicode-BoldItalic.otf",
        ],
    },
    Typeface {
        key: "eb garamond",
        name: "EB Garamond",
        files: [
            "EBGaramond-Regular.ttf",
            "EBGaramond-Italic.ttf",
            "EBGaramond-Bold.ttf",
            "EBGaramond-BoldItalic.ttf",
        ],
    },
    Typeface {
        key: "cardo",
        name: "Cardo",
        // Cardo has no bold italic: its bold stands in.
        files: [
            "Cardo-Regular.ttf",
            "Cardo-Italic.ttf",
            "Cardo-Bold.ttf",
            "Cardo-Bold.ttf",
        ],
    },
    Typeface {
        key: "literata",
        name: "Literata",
        files: [
            "Literata-Regular.ttf",
            "Literata-Italic.ttf",
            "Literata-Bold.ttf",
            "Literata-BoldItalic.ttf",
        ],
    },
    Typeface {
        key: "atkinson",
        name: "Atkinson Hyperlegible",
        files: [
            "AtkinsonHyperlegible-Regular.ttf",
            "AtkinsonHyperlegible-Italic.ttf",
            "AtkinsonHyperlegible-Bold.ttf",
            "AtkinsonHyperlegible-BoldItalic.ttf",
        ],
    },
];

/// Sizes by the printers' old names, in points.
pub const SIZES: [(&str, f32); 5] = [
    ("long primer", 10.0),
    ("small pica", 11.0),
    ("pica", 12.0),
    ("english", 14.0),
    ("great primer", 18.0),
];

/// The measure, as characters to a line. Full is the page's own margins.
pub const MEASURES: [(&str, f32); 4] = [
    ("narrow", 55.0),
    ("book", 66.0),
    ("wide", 75.0),
    ("full", 0.0),
];

/// Baseline to baseline, as a multiple of the size.
pub const LEADINGS: [(&str, f32); 3] = [("close", 1.25), ("book", 1.35), ("open", 1.5)];

/// How prose is set: justified, or ragged right.
pub const SETTINGS: [(&str, bool); 2] = [("justified", true), ("ragged", false)];

/// The letters a language needs, for saying which faces can set it. Rare
/// marks an edition might use (the Tironian ⁊) are left to the check of a
/// book's own letters.
pub const LANGUAGES: [(&str, &str); 3] = [
    ("old english", "æÆþÞðÐƿǷȝȜāēīōūȳǣĀĒĪŌŪȲǢċĊġĠ"),
    ("old norse", "ǫǪøØœŒęĘǿǾáéíóúýÁÉÍÓÚÝǽ"),
    ("greek", "αβγδεζηθικλμνξοπρσςτυφχψωΑΒΓΔΩάέήίόύώἀἁὰᾶῆῶ"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub face: String,
    pub size: String,
    pub measure: String,
    pub leading: String,
    pub setting: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            face: "junicode".into(),
            size: "small pica".into(),
            measure: "full".into(),
            leading: "book".into(),
            setting: "justified".into(),
        }
    }
}

const FILE: &str = "wordhord.toml";

impl Settings {
    /// The settings in the app folder, or the defaults.
    pub fn load(app: &Path) -> Settings {
        std::fs::read_to_string(app.join(FILE))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, app: &Path) -> Result<(), String> {
        let text = format!(
            "# Wordhord's reading settings. Faces: {}. Sizes: {}.\n\
             # Measures: {}. Leadings: {}. Settings: {}.\n{}",
            FACES.map(|f| f.key).join(", "),
            SIZES.map(|s| s.0).join(", "),
            MEASURES.map(|m| m.0).join(", "),
            LEADINGS.map(|l| l.0).join(", "),
            SETTINGS.map(|s| s.0).join(", "),
            toml::to_string(self).map_err(|e| e.to_string())?
        );
        let path = app.join(FILE);
        let part = path.with_extension("toml.part");
        std::fs::write(&part, text)
            .and_then(|_| std::fs::rename(&part, &path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The face chosen, or Junicode.
    pub fn typeface(&self) -> &'static Typeface {
        FACES
            .iter()
            .find(|f| f.key == self.face)
            .unwrap_or(&FACES[0])
    }

    fn pick<T: Copy>(list: &[(&str, T)], name: &str, default: usize) -> T {
        list.iter()
            .find(|(n, _)| *n == name)
            .map_or(list[default].1, |(_, v)| *v)
    }

    /// The profile with these settings, and its fonts. The margins are
    /// widened to bring the line to its measure, never narrowed below the
    /// profile's own.
    pub fn apply(&self, base: &Profile) -> Result<(Profile, Fonts), String> {
        let mut p = base.clone();
        let [regular, italic, bold, bold_italic] =
            self.typeface().files.map(|f| format!("fonts/{f}"));
        p.fonts.regular = regular;
        p.fonts.italic = italic;
        p.fonts.bold = bold;
        p.fonts.bold_italic = bold_italic;
        p.body_size = Settings::pick(&SIZES, &self.size, 1);
        p.leading = Settings::pick(&LEADINGS, &self.leading, 1);
        p.justify = Settings::pick(&SETTINGS, &self.setting, 0);
        let fonts = Fonts::load(&p.dir, &p.fonts)?;
        let chars = Settings::pick(&MEASURES, &self.measure, 3);
        if chars > 0.0 {
            let alphabet: f32 = fonts
                .shape(
                    "abcdefghijklmnopqrstuvwxyz",
                    wordhord_typeset::fonts::Face::Regular,
                    p.pt(p.body_size),
                    &[],
                )
                .iter()
                .map(|s| s.width)
                .sum();
            let measure = alphabet / 26.0 * chars;
            let side_px = (p.width as f32 - measure) / 2.0;
            let side = side_px * 25.4 / p.ppi;
            p.margins.left = p.margins.left.max(side);
            p.margins.right = p.margins.right.max(side);
        }
        Ok((p, fonts))
    }

    /// What tells one layout from another, for the page cache.
    pub fn key(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.typeface().key,
            self.size,
            self.measure,
            self.leading,
            self.setting
        )
    }
}

/// The languages `fonts` can set, by name.
pub fn languages(fonts: &Fonts) -> Vec<&'static str> {
    LANGUAGES
        .iter()
        .filter(|(_, letters)| letters.chars().all(|c| fonts.covers(c)))
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_read_as_the_defaults() {
        let s: Settings = toml::from_str("face = \"comic sans\"\nsize = \"pica\"").unwrap();
        assert_eq!(s.typeface().key, "junicode");
        assert_eq!(Settings::pick(&SIZES, &s.size, 1), 12.0);
        assert_eq!(Settings::pick(&MEASURES, &s.measure, 3), 0.0);
    }
}
