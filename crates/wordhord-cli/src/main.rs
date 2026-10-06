//! `wordhord check <book>…` and `wordhord align <translation> [<original>]`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use wordhord_book::align::align;
use wordhord_book::model::BlockKind;
use wordhord_book::{Book, Diagnostic};

const USAGE: &str = "usage:
  wordhord check <book>...
  wordhord align <translation> [<original>]

A book is a folder with book.toml and text/*.dj. The original defaults to
the folder beside the translation named by its `original`.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() > 1 => check(&args[1..]),
        Some("align") if args.len() == 2 || args.len() == 3 => align_books(&args[1], args.get(2)),
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
