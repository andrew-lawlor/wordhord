//! A book on the device. Either an EPUB, laid out here chapter by chapter
//! the first time each is opened and cached; or an edition compiled on the
//! desktop, one chapter of pages ready to draw.
//!
//! The app folder holds what layout needs: `profile.toml` (the screen,
//! margins and type) with the fonts it names, and `cache/`, where each
//! EPUB's pages are kept by the book's hash and the layout settings.

use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};
use wordhord_epub::Epub;
use wordhord_typeset::fonts::{FontFile, Fonts};
use wordhord_typeset::page::Typeset;
use wordhord_typeset::profile::Profile;

/// Bumped when layout changes, so cached pages are laid out again.
const LAYOUT: &str = "1";

pub struct Book {
    /// Where the book's state lives: its position, and for an EPUB its
    /// cached pages.
    pub dir: PathBuf,
    pub name: String,
    /// The book's file or folder, and the app folder: enough to open it
    /// again, on another thread.
    pub path: PathBuf,
    pub app: PathBuf,
    pub fonts: Fonts,
    source: Source,
}

enum Source {
    Compiled(Typeset),
    Epub {
        epub: Box<Epub>,
        profile: Box<Profile>,
        /// The documents that make up the reading order.
        chapters: Vec<String>,
        lang: String,
        /// Where this layout's pages are cached.
        pages: PathBuf,
    },
}

impl Book {
    /// Opens the book at `path`: an `.epub`, or a folder with `pages.json`.
    pub fn open(path: &Path, app: &Path) -> Result<Book, String> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.join("pages.json").exists() {
            let mut book = Book::compiled(path, &app.join("fonts"), name)?;
            book.app = app.to_path_buf();
            return Ok(book);
        }
        let started = Instant::now();
        let profile_path = app.join("profile.toml");
        let profile = Profile::load(&profile_path)?;
        let fonts = Fonts::load(&profile.dir, &profile.fonts)?;
        let epub = Epub::open(path)?;
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let book = hex(&Sha256::digest(&bytes))[..16].to_string();
        let mut settings = Sha256::new();
        settings.update(std::fs::read(&profile_path).unwrap_or_default());
        settings.update(LAYOUT);
        let key = hex(&settings.finalize())[..12].to_string();
        let dir = app.join("cache").join(&book);
        let chapters = epub
            .spine
            .iter()
            .filter(|s| s.linear)
            .map(|s| s.path.clone())
            .collect();
        let lang = epub.meta.language.clone().unwrap_or_else(|| "en".into());
        eprintln!("{name}: opened and hashed in {:?}", started.elapsed());
        Ok(Book {
            name,
            path: path.to_path_buf(),
            app: app.to_path_buf(),
            fonts,
            source: Source::Epub {
                epub: Box::new(epub),
                profile: Box::new(profile),
                chapters,
                lang,
                pages: dir.join("pages").join(key),
            },
            dir,
        })
    }

    /// Pages compiled on the desktop. Each font is found by its file name in
    /// `fonts`, and must have the hash the pages were set with: a glyph ID
    /// means nothing in any other font.
    fn compiled(dir: &Path, fonts: &Path, name: String) -> Result<Book, String> {
        let json = std::fs::read(dir.join("pages.json"))
            .map_err(|e| format!("{}: {e}", dir.join("pages.json").display()))?;
        let typeset: Typeset =
            serde_json::from_slice(&json).map_err(|e| format!("pages.json: {e}"))?;
        let mut files = Vec::new();
        for f in &typeset.fonts {
            let file = Path::new(&f.path)
                .file_name()
                .ok_or_else(|| format!("{}: no file name", f.path))?;
            let path = fonts.join(file);
            let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let sha256 = hex(&Sha256::digest(&data));
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
            name,
            path: dir.to_path_buf(),
            app: PathBuf::new(),
            fonts: Fonts::from_files(files)?,
            source: Source::Compiled(typeset),
        })
    }

    pub fn chapters(&self) -> usize {
        match &self.source {
            Source::Compiled(_) => 1,
            Source::Epub { chapters, .. } => chapters.len(),
        }
    }

    /// A chapter's pages: from the cache, or laid out now and cached.
    pub fn chapter(&self, k: usize) -> Result<Typeset, String> {
        let (epub, profile, chapters, lang, pages) = match &self.source {
            Source::Compiled(t) => return Ok(t.clone()),
            Source::Epub {
                epub,
                profile,
                chapters,
                lang,
                pages,
            } => (epub, profile, chapters, lang, pages),
        };
        let path = chapters.get(k).ok_or_else(|| format!("no chapter {k}"))?;
        let cached = pages.join(format!("{k:03}.json"));
        let started = Instant::now();
        if let Ok(json) = std::fs::read(&cached) {
            if let Ok(t) = serde_json::from_slice::<Typeset>(&json) {
                eprintln!(
                    "chapter {k}: {} pages from the cache in {:?}",
                    t.pages.len(),
                    started.elapsed()
                );
                return Ok(t);
            }
        }
        let blocks = wordhord_epub::convert::chapter(epub, path)?;
        let converted = started.elapsed();
        let read = |src: &str| epub.bytes(src).ok();
        let t = wordhord_typeset::lay_out(&blocks, lang, profile, &self.fonts, &[], Some(&read));
        let laid_out = started.elapsed();
        let _ = std::fs::create_dir_all(pages);
        if let Ok(json) = serde_json::to_vec(&t) {
            // Written whole, then renamed: another thread may be reading.
            let partial = cached.with_extension("part");
            if std::fs::write(&partial, json).is_ok() {
                let _ = std::fs::rename(&partial, &cached);
            }
        }
        eprintln!(
            "chapter {k} ({path}): converted in {converted:?}, {} pages laid out in {:?}, cached in {:?}",
            t.pages.len(),
            laid_out - converted,
            started.elapsed() - laid_out
        );
        Ok(t)
    }

    /// The book's table of contents: each entry's label, depth, and the
    /// chapter it opens. Entries in documents outside the reading order
    /// are left out.
    pub fn contents(&self) -> Vec<(String, usize, usize)> {
        let Source::Epub { epub, chapters, .. } = &self.source else {
            return Vec::new();
        };
        let mut out: Vec<(String, usize, usize)> = epub
            .toc
            .iter()
            .filter_map(|t| {
                let file = t.target.split('#').next()?;
                let chapter = chapters.iter().position(|c| c == file)?;
                Some((t.label.clone(), t.depth, chapter))
            })
            .collect();
        // Without usable contents, the chapters themselves, numbered.
        if out.is_empty() {
            out = (0..chapters.len())
                .map(|k| (format!("Chapter {}", k + 1), 0, k))
                .collect();
        }
        out
    }

    /// The book's title, as its package gives it.
    pub fn title(&self) -> String {
        match &self.source {
            Source::Epub { epub, .. } if !epub.meta.title.is_empty() => epub.meta.title.clone(),
            _ => self.name.clone(),
        }
    }

    /// Whether a chapter's pages are already laid out and kept.
    pub fn is_cached(&self, k: usize) -> bool {
        match &self.source {
            Source::Compiled(_) => true,
            Source::Epub { pages, .. } => pages.join(format!("{k:03}.json")).exists(),
        }
    }

    /// Where finished images are kept.
    pub fn picture_dir(&self) -> PathBuf {
        match &self.source {
            Source::Compiled(_) => self.dir.join("pictures"),
            Source::Epub { pages, .. } => pages.join("pictures"),
        }
    }

    /// Makes every image a chapter's pages draw, ready to draw.
    pub fn prepare_pictures(&self, t: &Typeset) {
        let dir = self.picture_dir();
        let read = |src: &str| self.image(src);
        for img in t.pages.iter().flat_map(|p| &p.images) {
            let (w, h) = (img.width.round() as u32, img.height.round() as u32);
            if !wordhord_typeset::render::picture_path(&dir, &img.src, w, h).exists() {
                let _ = wordhord_typeset::render::cached_picture(Some(&dir), &img.src, w, h, &read);
            }
        }
    }

    /// An image's bytes, by the source its pages give.
    pub fn image(&self, src: &str) -> Option<Vec<u8>> {
        match &self.source {
            Source::Compiled(_) => std::fs::read(self.dir.join("images").join(src)).ok(),
            Source::Epub { epub, .. } => epub.bytes(src).ok(),
        }
    }

    /// Where the reader left off: chapter and page.
    pub fn position(&self) -> (usize, usize) {
        let text = std::fs::read_to_string(self.dir.join("position")).unwrap_or_default();
        let mut numbers = text
            .split_whitespace()
            .filter_map(|n| n.parse::<usize>().ok());
        match (numbers.next(), numbers.next()) {
            (Some(chapter), Some(page)) => (chapter.min(self.chapters().saturating_sub(1)), page),
            // Draft files kept only the page.
            (Some(page), None) => (0, page),
            _ => (0, 0),
        }
    }

    pub fn remember(&self, chapter: usize, page: usize) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join("position"), format!("{chapter} {page}\n"));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
