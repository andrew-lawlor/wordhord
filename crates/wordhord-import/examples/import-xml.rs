//! A one-off: import XML files (Pettit's TEI edition of the Poetic Edda)
//! with a recipe, the way `wordhord import` imports an EPUB. Not part of
//! the product, which takes EPUBs only.
//!
//!   cargo run -q -p wordhord-import --example import-xml -- \
//!     <recipe.toml> <new folder> <file.xml>...

use std::path::Path;
use std::process::ExitCode;

use wordhord_import::epub::Doc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: import-xml <recipe.toml> <new folder> <file.xml>...");
        return ExitCode::from(2);
    }
    let run = || -> Result<bool, String> {
        let recipe = wordhord_import::load_recipe(Path::new(&args[0]))?;
        let mut docs = Vec::new();
        for f in &args[2..] {
            let text = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
            docs.push(Doc {
                href: f.clone(),
                text,
            });
        }
        let imported = wordhord_import::import_documents(docs, &recipe)?;
        let r = &imported.report;
        for ch in &imported.chapters {
            let stanzas = ch
                .blocks
                .iter()
                .filter(|b| matches!(b, wordhord_import::convert::Out::Stanza { .. }))
                .count();
            println!(
                "{}: {} blocks, {stanzas} stanzas",
                ch.prefix,
                ch.blocks.len()
            );
        }
        for (key, (n, sample)) in &r.unmapped {
            println!("unmapped class {key} ×{n}: {sample}");
        }
        for (key, (n, sample)) in &r.unhandled {
            println!("not handled {key} ×{n}: {sample}");
        }
        for p in &r.problems {
            println!("problem: {p}");
        }
        let out = Path::new(&args[1]);
        wordhord_import::write(&imported, &recipe, out)?;
        let (book, differences) = wordhord_import::verify(&imported, out)?;
        for d in &differences {
            println!("read back differently: {d}");
        }
        for d in &book.diagnostics {
            eprintln!("{}/{d}", out.display());
        }
        println!(
            "wrote {}: {} differences on reading back; check: {} errors, {} warnings",
            out.display(),
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
