//! The profile's fonts, loaded once and pinned by hash, and text shaped
//! with them. Text is split into runs by which font has each character,
//! so a character Junicode lacks falls back to the next font.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::profile::FontPaths;

/// Which face of the family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Italic,
    Bold,
    BoldItalic,
    /// For code. Italic and bold stay monospace.
    Mono,
}

impl Face {
    pub fn italic(self) -> Face {
        match self {
            Face::Regular => Face::Italic,
            Face::Bold => Face::BoldItalic,
            f => f,
        }
    }

    pub fn bold(self) -> Face {
        match self {
            Face::Regular => Face::Bold,
            Face::Italic => Face::BoldItalic,
            f => f,
        }
    }
}

pub struct FontFile {
    /// As given in the profile, relative to the project.
    pub path: String,
    pub sha256: String,
    /// The file, kept for the life of the program: the parsed faces
    /// borrow it.
    pub data: &'static [u8],
}

/// The family's four faces and a fallback, as indices into `files`.
pub struct Fonts {
    pub files: Vec<FontFile>,
    /// Each file parsed once: parsing per lookup made shaping crawl.
    parsed: Vec<rustybuzz::Face<'static>>,
    faces: [usize; 5],
    fallback: Option<usize>,
}

/// One glyph, positioned relative to the start of its run, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    /// Byte offset of the character it came from, in the shaped text.
    pub cluster: u32,
}

/// Text shaped in one font: its glyphs and its width.
#[derive(Debug, Clone, PartialEq)]
pub struct Shaped {
    pub font: usize,
    pub size: f32,
    pub glyphs: Vec<Glyph>,
    pub width: f32,
}

impl Fonts {
    /// The profile's fonts, relative to `root`. Without a monospace face,
    /// code is set in the regular one.
    pub fn load(root: &Path, paths: &FontPaths) -> Result<Fonts, String> {
        let mut files: Vec<FontFile> = Vec::new();
        let mut add = |path: &str| -> Result<usize, String> {
            if let Some(k) = files.iter().position(|f| f.path == path) {
                return Ok(k);
            }
            let data: &'static [u8] = Box::leak(
                std::fs::read(root.join(path))
                    .map_err(|e| format!("{path}: {e}"))?
                    .into_boxed_slice(),
            );
            rustybuzz::Face::from_slice(data, 0).ok_or_else(|| format!("{path}: not a font"))?;
            let sha256 = Sha256::digest(data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            files.push(FontFile {
                path: path.to_string(),
                sha256,
                data,
            });
            Ok(files.len() - 1)
        };
        let regular = add(&paths.regular)?;
        let faces = [
            regular,
            add(&paths.italic)?,
            add(&paths.bold)?,
            add(&paths.bold_italic)?,
            match &paths.mono {
                Some(m) => add(m)?,
                None => regular,
            },
        ];
        let fallback = paths.fallback.as_deref().map(&mut add).transpose()?;
        let parsed = files
            .iter()
            .map(|f| rustybuzz::Face::from_slice(f.data, 0).expect("checked when read"))
            .collect();
        Ok(Fonts {
            files,
            parsed,
            faces,
            fallback,
        })
    }

    /// Fonts as a compiled book lists them, already read: what the device
    /// draws with. The faces are the first four files, as typeset.
    pub fn from_files(files: Vec<FontFile>) -> Result<Fonts, String> {
        let parsed = files
            .iter()
            .map(|f| {
                rustybuzz::Face::from_slice(f.data, 0)
                    .ok_or_else(|| format!("{}: not a font", f.path))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let last = files.len().checked_sub(1).ok_or("no fonts")?;
        Ok(Fonts {
            files,
            parsed,
            faces: [0, 1.min(last), 2.min(last), 3.min(last), 0],
            fallback: None,
        })
    }

    pub fn file(&self, face: Face) -> usize {
        self.faces[face as usize]
    }

    fn rb(&self, file: usize) -> &rustybuzz::Face<'static> {
        &self.parsed[file]
    }

    /// Units per em of a font file.
    pub fn units_per_em(&self, file: usize) -> f32 {
        self.rb(file).units_per_em() as f32
    }

    /// Ascender and descender of the regular face, in pixels at `size`.
    pub fn ascent_descent(&self, size: f32) -> (f32, f32) {
        let f = self.rb(self.faces[0]);
        let scale = size / f.units_per_em() as f32;
        (f.ascender() as f32 * scale, -(f.descender() as f32) * scale)
    }

    /// Shapes `text` in `face` at `size` pixels, with OpenType `features`
    /// (`smcp` for small caps). Returns one run per font used: characters
    /// the face lacks go to the fallback.
    pub fn shape(&self, text: &str, face: Face, size: f32, features: &[&str]) -> Vec<Shaped> {
        let text = visible(text);
        if text.is_empty() {
            return Vec::new();
        }
        let primary = self.file(face);
        let has =
            |file: usize, c: char| self.rb(file).glyph_index(c).is_some() || c.is_whitespace();
        // Split into runs by font.
        let mut runs: Vec<(usize, String)> = Vec::new();
        for c in text.chars() {
            let file = match self.fallback {
                Some(fb) if !has(primary, c) && has(fb, c) => fb,
                _ => primary,
            };
            match runs.last_mut() {
                Some((f, s)) if *f == file => s.push(c),
                _ => runs.push((file, c.to_string())),
            }
        }
        let features: Vec<rustybuzz::Feature> =
            features.iter().filter_map(|f| f.parse().ok()).collect();
        let mut offset = 0u32;
        runs.into_iter()
            .map(|(file, s)| {
                let face = self.rb(file);
                let scale = size / face.units_per_em() as f32;
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&s);
                buffer.set_direction(rustybuzz::Direction::LeftToRight);
                let out = rustybuzz::shape(face, &features, buffer);
                let mut x = 0.0;
                let mut glyphs = Vec::with_capacity(out.len());
                for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                    glyphs.push(Glyph {
                        id: info.glyph_id as u16,
                        x: x + pos.x_offset as f32 * scale,
                        y: -(pos.y_offset as f32) * scale,
                        cluster: offset + info.cluster,
                    });
                    x += pos.x_advance as f32 * scale;
                }
                offset += s.len() as u32;
                Shaped {
                    font: file,
                    size,
                    glyphs,
                    width: x,
                }
            })
            .collect()
    }

    /// The width of a space in the regular face at `size`.
    pub fn space(&self, size: f32) -> f32 {
        self.shape(" ", Face::Regular, size, &[])
            .iter()
            .map(|s| s.width)
            .sum()
    }
}

/// Text without the invisible format characters editions put in it
/// (zero-width no-break spaces before dashes, direction marks): they'd
/// shape as empty boxes.
pub fn visible(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{feff}'
                    | '\u{200b}'
                    | '\u{200c}'
                    | '\u{200d}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{2060}'
                    | '\u{ad}'
            )
        })
        .collect()
}
