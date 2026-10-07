//! The books on the device, by what they are, not where they sit.
//!
//! Every EPUB under the root is found, wherever Calibre or anyone put it.
//! What each book is comes from, in order: Calibre's `metadata.calibre`
//! at the root of a Kobo it manages; a `metadata.opf` beside the book, as
//! in a Calibre library folder; the EPUB's own package. Books locked by DRM
//! are left out, since Wordhord can't read them.
//!
//! The index is kept in the app folder. A rescan opens only books that are
//! new or changed since.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wordhord_epub::Epub;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to the library's root.
    pub path: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
    pub title: String,
    pub title_sort: String,
    pub authors: Vec<String>,
    /// The first author, surname first: "Harrison, Harry".
    pub author_sort: String,
    pub series: Option<String>,
    pub series_index: Option<f32>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    /// The cover: an archive path, or `../cover.jpg` for one beside the book.
    pub cover: Option<String>,
    /// The cover's size in pixels, so it's drawn in proportion.
    #[serde(default)]
    pub cover_size: Option<(u32, u32)>,
    pub drm: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub books: Vec<Entry>,
}

impl Library {
    /// Finds every EPUB under `root`, reusing what `index` knew of books
    /// unchanged since, and saves the new index there.
    pub fn scan(root: &Path, index: &Path) -> Library {
        let started = Instant::now();
        let old: HashMap<String, Entry> = std::fs::read(index)
            .ok()
            .and_then(|b| serde_json::from_slice::<Library>(&b).ok())
            .map(|l| l.books.into_iter().map(|e| (e.path.clone(), e)).collect())
            .unwrap_or_default();
        let calibre = calibre_metadata(root);
        let mut files = Vec::new();
        find(root, root, &mut files);
        let (mut read, mut kept) = (0, 0);
        let mut books = Vec::new();
        for (path, size, modified) in files {
            let entry = match old.get(&path) {
                // Entries from before covers were measured are read again.
                Some(e)
                    if e.size == size
                        && e.modified == modified
                        && (e.cover.is_none() || e.cover_size.is_some()) =>
                {
                    kept += 1;
                    e.clone()
                }
                _ => {
                    read += 1;
                    match entry(root, &path, size, modified, calibre.get(&path)) {
                        Some(e) => e,
                        None => continue,
                    }
                }
            };
            if !entry.drm {
                books.push(entry);
            }
        }
        books.sort_by(|a, b| {
            a.title_sort
                .to_lowercase()
                .cmp(&b.title_sort.to_lowercase())
        });
        let library = Library { books };
        if let Some(dir) = index.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(&library) {
            let partial = index.with_extension("part");
            if std::fs::write(&partial, json).is_ok() {
                let _ = std::fs::rename(&partial, index);
            }
        }
        eprintln!(
            "library: {} books ({read} read, {kept} unchanged) in {:?}",
            library.books.len(),
            started.elapsed()
        );
        library
    }

    /// By author, surname first, A to Z; each author's books by series,
    /// then title.
    pub fn by_author(&self) -> Vec<(String, Vec<&Entry>)> {
        let mut groups: BTreeMap<String, (String, Vec<&Entry>)> = BTreeMap::new();
        for e in &self.books {
            let key = e.author_sort.to_lowercase();
            let name = e
                .authors
                .first()
                .cloned()
                .unwrap_or_else(|| "Unknown".into());
            groups
                .entry(key)
                .or_insert_with(|| (name, Vec::new()))
                .1
                .push(e);
        }
        groups
            .into_values()
            .map(|(name, mut books)| {
                books.sort_by(|a, b| series_order(a, b).then(a.title_sort.cmp(&b.title_sort)));
                (name, books)
            })
            .collect()
    }

    /// Series, A to Z, each in reading order.
    pub fn by_series(&self) -> Vec<(String, Vec<&Entry>)> {
        let mut groups: BTreeMap<String, (String, Vec<&Entry>)> = BTreeMap::new();
        for e in &self.books {
            if let Some(s) = &e.series {
                groups
                    .entry(sortable(s))
                    .or_insert_with(|| (s.clone(), Vec::new()))
                    .1
                    .push(e);
            }
        }
        groups
            .into_values()
            .map(|(name, mut books)| {
                books.sort_by(|a, b| series_order(a, b));
                (name, books)
            })
            .collect()
    }

    /// Tags, A to Z, each with its books by title. Only tags a person
    /// gave: library-catalogue subjects that publishers ship ("Henry V,
    /// King of England, 1387-1422 -- Drama") are left out.
    pub fn by_tag(&self) -> Vec<(String, Vec<&Entry>)> {
        let mut groups: BTreeMap<String, (String, Vec<&Entry>)> = BTreeMap::new();
        for e in &self.books {
            for t in e.tags.iter().filter(|t| is_own_tag(t)) {
                groups
                    .entry(t.to_lowercase())
                    .or_insert_with(|| (t.clone(), Vec::new()))
                    .1
                    .push(e);
            }
        }
        groups.into_values().collect()
    }
}

fn is_own_tag(tag: &str) -> bool {
    !tag.contains("--")
        && !tag.chars().next().is_some_and(|c| c.is_ascii_digit())
        && tag.chars().count() <= 40
}

impl Entry {
    /// The cover's bytes, from the book or from beside it.
    pub fn cover_bytes(&self, root: &Path) -> Option<Vec<u8>> {
        let cover = self.cover.as_deref()?;
        let full = root.join(&self.path);
        if cover == "../cover.jpg" {
            return std::fs::read(full.parent()?.join("cover.jpg")).ok();
        }
        Epub::open(&full).ok()?.bytes(cover).ok()
    }

    /// "Harry Harrison · The Hammer and the Cross 2"
    pub fn byline(&self) -> String {
        let author = self.authors.first().cloned().unwrap_or_default();
        match (&self.series, self.series_index) {
            (Some(s), Some(n)) => format!("{author} · {s} {}", number(n)),
            (Some(s), None) => format!("{author} · {s}"),
            _ => author,
        }
    }
}

/// 2 as "2", 2.5 as "2.5".
pub fn number(n: f32) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A book opened lately: where it is, when, and how far through.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opened {
    pub path: String,
    pub opened: u64,
    pub progress: f32,
}

/// Books opened lately, most recent first.
pub fn recent(app: &Path) -> Vec<Opened> {
    std::fs::read(app.join("recent.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Records a book as opened now, `progress` of the way through.
pub fn opened(app: &Path, path: &str, progress: f32) {
    let mut list = recent(app);
    list.retain(|o| o.path != path);
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    list.insert(
        0,
        Opened {
            path: path.to_string(),
            opened: now,
            progress,
        },
    );
    list.truncate(50);
    if let Ok(json) = serde_json::to_vec(&list) {
        let file = app.join("recent.json");
        let partial = file.with_extension("part");
        if std::fs::write(&partial, json).is_ok() {
            let _ = std::fs::rename(&partial, file);
        }
    }
}

fn series_order(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    let key = |e: &Entry| {
        (
            e.series.as_deref().map(sortable),
            e.series_index.unwrap_or(f32::MAX),
        )
    };
    let (ka, kb) = (key(a), key(b));
    ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
}

/// A title as it sorts: without a leading article, and lowercase.
pub fn sortable(title: &str) -> String {
    let lower = title.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

/// Every `.epub` under `dir`, as (path relative to `root`, size, mtime).
/// Hidden folders are skipped, except Kobo's own books, which are DRM's.
fn find(root: &Path, dir: &Path, out: &mut Vec<(String, u64, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        // Following links: a library may be linked in from elsewhere.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if !name.starts_with('.') {
                find(root, &path, out);
            }
        } else if name.to_lowercase().ends_with(".epub") {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            if let Ok(rel) = path.strip_prefix(root) {
                out.push((rel.to_string_lossy().into_owned(), meta.len(), modified));
            }
        }
    }
}

/// What Calibre's driver knows about the books it put on a Kobo, by path.
fn calibre_metadata(root: &Path) -> HashMap<String, CalibreBook> {
    std::fs::read(root.join("metadata.calibre"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<CalibreBook>>(&b).ok())
        .map(|books| books.into_iter().map(|b| (b.lpath.clone(), b)).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CalibreBook {
    lpath: String,
    title: Option<String>,
    title_sort: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    author_sort: Option<String>,
    series: Option<String>,
    series_index: Option<f32>,
    #[serde(default)]
    tags: Vec<String>,
}

fn entry(
    root: &Path,
    path: &str,
    size: u64,
    modified: u64,
    calibre: Option<&CalibreBook>,
) -> Option<Entry> {
    let full = root.join(path);
    let epub = Epub::open(&full).ok()?;
    let mut m = epub.meta.clone();
    // A Calibre library keeps the book's real metadata beside it.
    let beside = full.parent().map(|d| d.join("metadata.opf"));
    if let Some(text) = beside
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
    {
        if let Ok(doc) = wordhord_epub::xml::parse(&text) {
            let c = wordhord_epub::package::read_meta(&doc);
            m.title = some(c.title).unwrap_or(m.title);
            if !c.creators.is_empty() {
                m.creators = c.creators;
            }
            m.title_sort = c.title_sort.or(m.title_sort);
            m.author_sort = c.author_sort.or(m.author_sort);
            m.series = c.series.or(m.series);
            m.series_index = c.series_index.or(m.series_index);
            if !c.tags.is_empty() {
                m.tags = c.tags;
            }
            m.language = c.language.or(m.language);
        }
    }
    let beside_cover = full
        .parent()
        .map(|d| d.join("cover.jpg"))
        .filter(|p| p.exists());
    if let Some(c) = calibre {
        m.title = c.title.clone().and_then(some).unwrap_or(m.title);
        if !c.authors.is_empty() {
            m.creators = c.authors.clone();
        }
        m.title_sort = c.title_sort.clone().or(m.title_sort);
        m.author_sort = c.author_sort.clone().or(m.author_sort);
        m.series = c.series.clone().or(m.series);
        m.series_index = c.series_index.or(m.series_index);
        if !c.tags.is_empty() {
            m.tags = c.tags.clone();
        }
    }
    let title = if m.title.is_empty() {
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        m.title
    };
    let author_sort = m.author_sort.unwrap_or_else(|| {
        // "Harry Harrison" → "Harrison, Harry"
        m.creators
            .first()
            .map_or("Unknown".into(), |a| match a.rsplit_once(' ') {
                Some((first, last)) => format!("{last}, {first}"),
                None => a.clone(),
            })
    });
    let cover = if beside_cover.is_some() {
        Some("../cover.jpg".to_string())
    } else {
        m.cover
    };
    let cover_size = match (&cover, &beside_cover) {
        (Some(_), Some(file)) => std::fs::read(file).ok(),
        (Some(c), None) => epub.bytes(c).ok(),
        _ => None,
    }
    .and_then(|b| wordhord_typeset::render::image_size(&b));
    Some(Entry {
        path: path.to_string(),
        size,
        modified,
        title_sort: m
            .title_sort
            .map(|t| t.to_lowercase())
            .unwrap_or_else(|| sortable(&title)),
        title,
        authors: m.creators,
        author_sort,
        series: m.series,
        series_index: m.series_index,
        tags: m.tags,
        language: m.language,
        cover,
        cover_size,
        drm: epub.drm(),
    })
}

fn some(s: String) -> Option<String> {
    (!s.trim().is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal EPUB at `path`, with `opf_meta` in its package.
    fn epub(path: &Path, title: &str, opf_meta: &str, encrypted: bool) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut files = vec![
            ("mimetype", "application/epub+zip".to_string()),
            ("META-INF/container.xml", r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#.to_string()),
            ("content.opf", format!(r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf"><dc:title>{title}</dc:title>{opf_meta}</metadata><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="cover.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="c"/></spine></package>"#)),
            ("c.xhtml", "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Text.</p></body></html>".to_string()),
        ];
        if encrypted {
            files.push(("META-INF/encryption.xml", r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:CipherData><enc:CipherReference URI="c.xhtml"/></enc:CipherData></enc:EncryptedData></encryption>"#.to_string()));
        }
        for (name, text) in files {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn calibre_metadata_beside_the_book_wins_and_drm_is_left_out() {
        let root = std::env::temp_dir().join(format!("wordhord-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // A Calibre library: the book, and its real metadata beside it.
        let book = root.join("Harry Harrison/One King's Way (81)/One King's Way.epub");
        epub(
            &book,
            "One King's Way",
            r#"<dc:creator>Harry Harrison</dc:creator>"#,
            false,
        );
        std::fs::write(
            book.parent().unwrap().join("metadata.opf"),
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
               <dc:title>One King's Way</dc:title><dc:creator opf:file-as="Harrison, Harry">Harry Harrison</dc:creator>
               <dc:subject>Heathen</dc:subject>
               <meta name="calibre:series" content="The Hammer and the Cross"/><meta name="calibre:series_index" content="2"/>
               </metadata></package>"#,
        )
        .unwrap();
        // The series' first book, with only its EPUB 3 collection.
        epub(
            &root.join("a/The Hammer and the Cross.epub"),
            "The Hammer and the Cross",
            r##"<dc:creator id="a">Harry Harrison</dc:creator><meta property="belongs-to-collection" id="s">The Hammer and the Cross</meta><meta refines="#s" property="group-position">1</meta>"##,
            false,
        );
        epub(&root.join("b/Locked.epub"), "Locked", "", true);
        // Hidden folders are Kobo's own.
        epub(&root.join(".kobo/kepub/x.epub"), "Store Book", "", false);

        let index = root.join("app/library.json");
        let library = Library::scan(&root, &index);
        let titles: Vec<&str> = library.books.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(
            titles,
            ["The Hammer and the Cross", "One King's Way"],
            "sorted without 'The'"
        );
        let series = library.by_series();
        assert_eq!(series.len(), 1);
        let order: Vec<&str> = series[0].1.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(order, ["The Hammer and the Cross", "One King's Way"]);
        let kings_way = &library.books[1];
        assert_eq!(kings_way.author_sort, "Harrison, Harry");
        assert_eq!(kings_way.tags, ["Heathen"]);
        assert_eq!(
            library.books[0].author_sort, "Harrison, Harry",
            "made from the name"
        );
        assert_eq!(library.by_author().len(), 1);
        // The second scan reads nothing.
        let again = Library::scan(&root, &index);
        assert_eq!(again.books, library.books);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn articles_dont_count_in_sorting() {
        assert_eq!(sortable("The Hobbit"), "hobbit");
        assert_eq!(sortable("A Game of Thrones"), "game of thrones");
        assert_eq!(sortable("Beowulf"), "beowulf");
    }
}
