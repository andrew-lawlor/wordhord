//! A compiled chapter on the device: its pages, and the fonts they name,
//! checked against the hashes they were typeset with.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use wordhord_typeset::fonts::{FontFile, Fonts};
use wordhord_typeset::page::Typeset;

pub struct Book {
    pub dir: PathBuf,
    pub typeset: Typeset,
    pub fonts: Fonts,
}

impl Book {
    /// Reads `dir/pages.json`. Each font is found by its file name in
    /// `fonts`, and must have the hash the pages were set with: a glyph ID
    /// means nothing in any other font.
    pub fn load(dir: &Path, fonts: &Path) -> Result<Book, String> {
        let json = std::fs::read(dir.join("pages.json"))
            .map_err(|e| format!("{}: {e}", dir.join("pages.json").display()))?;
        let typeset: Typeset =
            serde_json::from_slice(&json).map_err(|e| format!("pages.json: {e}"))?;
        let mut files = Vec::new();
        for f in &typeset.fonts {
            let name = Path::new(&f.path)
                .file_name()
                .ok_or_else(|| format!("{}: no file name", f.path))?;
            let path = fonts.join(name);
            let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let sha256: String = Sha256::digest(&data)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if sha256 != f.sha256 {
                return Err(format!(
                    "{}: not the font these pages were set in",
                    path.display()
                ));
            }
            files.push(FontFile {
                path: f.path.clone(),
                sha256,
                data: Box::leak(data.into_boxed_slice()),
            });
        }
        Ok(Book {
            dir: dir.to_path_buf(),
            typeset,
            fonts: Fonts::from_files(files)?,
        })
    }

    /// The page to open on: where the reader left off.
    pub fn position(&self) -> usize {
        std::fs::read_to_string(self.dir.join("position"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
            .min(self.typeset.pages.len().saturating_sub(1))
    }

    pub fn remember(&self, page: usize) {
        let _ = std::fs::write(self.dir.join("position"), format!("{page}\n"));
    }
}

/// The book to open in a library folder: the one read last, or else the
/// first by name.
pub fn choose(library: &Path) -> Option<PathBuf> {
    let last = std::fs::read_to_string(library.join("last")).ok();
    if let Some(name) = last.as_deref().map(str::trim) {
        let dir = library.join(name);
        if dir.join("pages.json").exists() {
            return Some(dir);
        }
    }
    let mut books: Vec<PathBuf> = std::fs::read_dir(library)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("pages.json").exists())
        .collect();
    books.sort();
    books.into_iter().next()
}
