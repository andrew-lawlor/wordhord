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

`check` reports anything outside the vocabulary, missing or repeated IDs, unknown names, bad references, and Djot syntax that silently became text. `align` matches a translation to its original by canonical reference, line to line or stanza to stanza.
