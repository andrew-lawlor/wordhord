# Wordhord

A reader for old texts. The original and the translation, line by line. Write in the margins with a pen. Cite any passage exactly.

For stylus Kobos, launched from NickelMenu. Books are compiled on the desktop. See [SPEC.md](SPEC.md) and [VOCABULARY.md](VOCABULARY.md).

## What works so far

`wordhord-book` reads a book's source, numbers its verse and checks it against the vocabulary. `wordhord` is its command:

```
cargo run -q -- check fixtures/*/
cargo run -q -- align fixtures/beowulf-gummere
cargo test --workspace
```

`import` converts an EPUB by recipe (`recipes/`): a reviewed mapping from one edition's markup to the vocabulary. It reports anything the recipe doesn't explain, then reads its output back and compares every line, caesura and source page.

```
cargo run -q -- import <book.epub> recipes/edda-bellows.toml <new folder>
```

`books/` holds converted editions: Pettit's *Poetic Edda* (2023, CC BY-NC), Old Norse and English, aligned stanza for stanza; and Bellows' translation (1923). See `books/README.md` for licences.

```
cargo run -q -- align books/edda-pettit
```

`check` reports anything outside the vocabulary, missing or repeated IDs, unknown names, bad references, and Djot syntax that silently became text. `align` matches a translation to its original by canonical reference, line to line or stanza to stanza.
