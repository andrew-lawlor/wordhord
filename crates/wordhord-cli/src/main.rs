//! `wordhord check <book>…`, `wordhord align <translation> [<original>]` and
//! `wordhord import <epub> <recipe> <folder>`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use wordhord_book::align::align;
use wordhord_book::model::BlockKind;
use wordhord_book::{Book, Diagnostic};

const USAGE: &str = "usage:
  wordhord check <book>...
  wordhord align <translation> [<original>]
  wordhord import <epub> <recipe.toml> <new folder>
  wordhord typeset <book> <chapter> <profile.toml> <folder> [--no-preview] [--translation <book>]
  wordhord epub <book.epub>
  wordhord epub <book.epub> <document> <profile.toml> <folder>

A book is a folder with book.toml and text/*.dj. The original defaults to
the folder beside the translation named by its `original`.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() > 1 => check(&args[1..]),
        Some("epub") if args.len() == 2 => epub_list(&args[1]),
        Some("epub") if args.len() == 5 => epub_typeset(&args[1], &args[2], &args[3], &args[4]),
        Some("align") if args.len() == 2 || args.len() == 3 => align_books(&args[1], args.get(2)),
        Some("import") if args.len() == 4 => import(&args[1], &args[2], &args[3]),
        Some("typeset") if args.len() >= 5 => {
            let mut previews = true;
            let mut translation = None;
            let mut rest = args[5..].iter();
            while let Some(flag) = rest.next() {
                match flag.as_str() {
                    "--no-preview" => previews = false,
                    "--translation" => match rest.next() {
                        Some(t) => translation = Some(t.as_str()),
                        None => {
                            eprintln!("{USAGE}");
                            return ExitCode::from(2);
                        }
                    },
                    _ => {
                        eprintln!("{USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            typeset(
                &args[1],
                &args[2],
                &args[3],
                &args[4],
                previews,
                translation,
            )
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn load(dir: &Path) -> Result<Book, ExitCode> {
    Book::load(dir).map_err(|d| {
        eprintln!("{}/{d}", dir.display());
        ExitCode::FAILURE
    })
}

fn print(dir: &Path, diagnostics: &[Diagnostic]) {
    for d in diagnostics {
        eprintln!("{}/{d}", dir.display());
    }
}

fn check(dirs: &[String]) -> ExitCode {
    let mut failed = false;
    for dir in dirs {
        let dir = Path::new(dir);
        let book = match load(dir) {
            Ok(b) => b,
            Err(_) => {
                failed = true;
                continue;
            }
        };
        print(dir, &book.diagnostics);
        let blocks = book.blocks();
        let lines: usize = blocks
            .iter()
            .map(|(_, b)| match &b.kind {
                BlockKind::Verse(v) | BlockKind::Stanza(v) => v.lines.len(),
                _ => 0,
            })
            .sum();
        println!(
            "{}: {} chapters, {} blocks, {} verse lines, {} names; {} errors, {} warnings",
            book.meta.id,
            book.chapters.len(),
            blocks.len(),
            lines,
            book.names.len(),
            book.errors(),
            book.warnings()
        );
        failed |= book.errors() > 0;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn align_books(translation: &str, original: Option<&String>) -> ExitCode {
    let tdir = Path::new(translation);
    let Ok(t) = load(tdir) else {
        return ExitCode::FAILURE;
    };
    let odir: PathBuf = match (original, &t.meta.original) {
        (Some(o), _) => o.into(),
        (None, Some(id)) => tdir.parent().unwrap_or(Path::new(".")).join(id),
        (None, None) => {
            eprintln!("{}: book.toml names no original; give one", tdir.display());
            return ExitCode::from(2);
        }
    };
    let Ok(o) = load(&odir) else {
        return ExitCode::FAILURE;
    };
    print(tdir, &t.diagnostics);
    print(&odir, &o.diagnostics);
    let a = align(&t, &o);
    let width = a.pairs.iter().map(|p| p.reference.len()).max().unwrap_or(0);
    for p in &a.pairs {
        for (k, line) in p.translation.iter().enumerate() {
            let r = if k == 0 { p.reference.as_str() } else { "" };
            println!("{r:width$}  {line}");
        }
        for line in &p.original {
            println!("{:width$}    {line}", "");
        }
        if p.translation.len() > 1 || p.original.len() > 1 {
            println!();
        }
    }
    print(tdir, &a.diagnostics);
    let errors = t.errors()
        + o.errors()
        + a.diagnostics
            .iter()
            .filter(|d| d.severity == wordhord_book::Severity::Error)
            .count();
    println!("{} pairs; {errors} errors", a.pairs.len());
    if errors > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn import(epub: &str, recipe: &str, out: &str) -> ExitCode {
    let run = || -> Result<bool, String> {
        let recipe = wordhord_import::load_recipe(Path::new(recipe))?;
        let imported = wordhord_import::import(Path::new(epub), &recipe)?;
        let r = &imported.report;
        for ch in &imported.chapters {
            use wordhord_import::convert::Out;
            let count = |f: fn(&Out) -> bool| ch.blocks.iter().filter(|b| f(b)).count();
            let stanzas = count(|b| matches!(b, Out::Stanza { .. }));
            let verse = count(|b| matches!(b, Out::Verse { .. }));
            let notes = count(|b| matches!(b, Out::Commentary { .. }));
            println!(
                "{}: {} blocks, {stanzas} stanzas, {verse} verse blocks, {notes} notes, {} footnotes",
                ch.prefix,
                ch.blocks.len(),
                ch.notes.len()
            );
        }
        println!(
            "{} source pages; {} stanzas continue after a change of speaker; {} passages of verse the edition didn't number",
            r.pages, r.continued, r.unnumbered
        );
        for (key, (n, sample)) in &r.unmapped {
            println!("unmapped class {key} ×{n}: {sample}");
        }
        for (key, (n, sample)) in &r.unhandled {
            println!("not handled {key} ×{n}: {sample}");
        }
        for p in &r.problems {
            println!("problem: {p}");
        }
        wordhord_import::write(&imported, &recipe, Path::new(out))?;
        let (book, differences) = wordhord_import::verify(&imported, Path::new(out))?;
        for d in &differences {
            println!("read back differently: {d}");
        }
        print(Path::new(out), &book.diagnostics);
        println!(
            "wrote {out}: {} differences on reading back; check: {} errors, {} warnings",
            differences.len(),
            book.errors(),
            book.warnings()
        );
        Ok(differences.is_empty() && book.errors() == 0)
    };
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn typeset(
    book: &str,
    chapter: &str,
    profile: &str,
    out: &str,
    previews: bool,
    translation: Option<&str>,
) -> ExitCode {
    let run = || -> Result<(), String> {
        let book = Book::load(Path::new(book)).map_err(|d| d.to_string())?;
        let chapter: usize = chapter
            .parse()
            .map_err(|_| format!("`{chapter}` isn't a chapter number"))?;
        let profile = wordhord_typeset::profile::Profile::load(Path::new(profile))?;
        let translation = translation
            .map(|t| Book::load(Path::new(t)).map_err(|d| d.to_string()))
            .transpose()?;
        let t = wordhord_typeset::typeset_chapter(
            &book,
            chapter,
            &profile,
            Path::new(out),
            previews,
            translation.as_ref(),
        )?;
        let words: usize = t.pages.iter().map(|p| p.words.len()).sum();
        println!(
            "{} pages, {words} words, {} notes, for {} ({}×{}); {} in {out}",
            t.pages.len(),
            t.notes.len(),
            t.profile,
            t.width,
            t.height,
            if previews {
                "pages.json, page-NNN.png and notes/"
            } else {
                "pages.json"
            }
        );
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// An EPUB's documents in reading order, with what the contents call them.
fn epub_list(path: &str) -> ExitCode {
    let epub = match wordhord_epub::Epub::open(Path::new(path)) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{} — {}", epub.meta.title, epub.meta.creators.join(", "));
    for (k, item) in epub.spine.iter().enumerate() {
        let label = epub
            .toc
            .iter()
            .find(|t| t.target.split('#').next() == Some(item.path.as_str()))
            .map_or("", |t| t.label.as_str());
        let size = epub.bytes(&item.path).map_or(0, |b| b.len());
        println!("{:>4}  {:>7} B  {}  {label}", k + 1, size, item.path);
    }
    ExitCode::SUCCESS
}

/// One document of an EPUB, converted and typeset, with previews.
fn epub_typeset(path: &str, document: &str, profile: &str, out: &str) -> ExitCode {
    let run = || -> Result<(), String> {
        let epub = wordhord_epub::Epub::open(Path::new(path))?;
        let n: usize = document
            .parse()
            .map_err(|_| format!("`{document}` isn't a document number"))?;
        let item = epub
            .spine
            .get(n.wrapping_sub(1))
            .ok_or_else(|| format!("the book has {} documents", epub.spine.len()))?;
        let started = std::time::Instant::now();
        let blocks = wordhord_epub::convert::chapter(&epub, &item.path)?;
        let converted = started.elapsed();
        let profile = wordhord_typeset::profile::Profile::load(Path::new(profile))?;
        let lang = epub.meta.language.as_deref().unwrap_or("en");
        let read = |src: &str| epub.bytes(src).ok();
        let t = wordhord_typeset::typeset_blocks(
            &blocks,
            lang,
            &profile,
            Path::new(out),
            true,
            &[],
            Some(&read),
        )?;
        let words: usize = t.pages.iter().map(|p| p.words.len()).sum();
        println!(
            "{}: {} blocks in {converted:?}; {} pages, {words} words, {} notes, in {:?}; previews in {out}",
            item.path,
            blocks.len(),
            t.pages.len(),
            t.notes.len(),
            started.elapsed()
        );
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
