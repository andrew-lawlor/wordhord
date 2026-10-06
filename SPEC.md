# Wordhord: spec

Draft 1, 2026-10-06.

> A reader for old texts. The original and the translation, line by line. Write in the margins with a pen. Cite any passage exactly.

*Wordhord* is Old English for "word-hoard", a store of words. In *Beowulf* (line 259) the leader of the Geats, asked who he is, *wordhord onleac*: unlocked his word-hoard.

## 1. Goal

A reader for the study of classical and medieval texts: originals and their translations, read together. Built for writing on the page. Books are compiled on the desktop. The Kobo draws pages and records ink. Every word's place on every page is known before you open the book.

**Target:** stylus Kobos (Elipsa 2E, Libra Colour), stock firmware. The reader is launched from NickelMenu and hands back to Nickel when it exits. Nickel stays installed and in charge of everything else: the store, USB, updates, sleep when the reader isn't running.

Not goals:
- Speed. E-ink refresh dwarfs layout time. Speed is not the pitch.
- Store books. DRM stays out, as with KOReader.
- Every EPUB's design. Books whose layout is the point (textbooks, comics, cookbooks) belong in another reader.

## 2. Principles

- **Record, don't infer.** Positions are known at build time. Nothing is guessed later.
- **The device stays simple.** It draws glyphs where it's told, turns pages and records strokes. It never lays out text.
- **Builds are deterministic.** The same source, fonts and profile give the same pages, byte for byte.
- **Text is source.** A book is plain text in a folder, under git. Fixes are commits.

## 3. Pipeline

```
EPUB ──import──▶ source (Djot + vocabulary, book.toml)
                   │
                   ▼ typeset (desktop, per device profile)
                 artifact (pages, word map, links)
                   │
                   ▼ copy to device
                 Kobo reader ──▶ annotations (append-only)
                                   │
                                   ▼ copy back
                                 Kollate
```

## 4. Source

A book is a folder:

```
odyssey/
  book.toml        metadata, source edition, reference scheme, originals
  names.toml       name register: people, places, groups (optional)
  text/*.dj        chapters, in Djot with the Wordhord vocabulary
  images/          figures, as imported
```

- Syntax is [Djot](https://djot.net). Parser: `jotdown` (Rust).
- Attributes sit on the line before a block. Values with dots are quoted. See the syntax rules in `VOCABULARY.md`.
- Meaning comes from the vocabulary (`VOCABULARY.md`), drawn from TEI.
- Every block gets a stable ID at import. IDs are never reused or renumbered.
- A validator rejects anything outside the vocabulary. The typesetter never meets a surprise.

### Import

The converter turns EPUB XHTML into source. It keeps meaning (verse, notes, small caps, languages) and drops publisher styling. What it can't classify, it flags for a human. It never guesses silently.

Test set: three hard books, including a verse epic.

## 5. Parallel texts

An original and its translations are separate books. They align by canonical reference: *Beowulf* line 1, *Völuspá* stanza 1.

- Each text declares its reference scheme in `book.toml`.
- Verse lines are counted, not tagged: `start=1` numbers the first line, the compiler numbers the rest.
- Where a line covers two of the original's, an override says so: `{n=12-13}`.
- Where lines don't match but stanzas do, align by stanza.

The artifact records, for each line or stanza, the reference it matches. How the original appears (popover, toggle, interleaved page) is the reader's choice, not the source's.

The same references are the keys for the research corpus. One format serves both.

## 5a. Citation

Three things make a passage citable, and the vocabulary carries all three:

- **Canonical references**, for verse and prose: *Beowulf* 4–5, *Völuspá* 1, *Natural History* 12.42.
- **Source pages**, from the printed edition: Gummere 1910, p. 2.
- **Names**, keyed to a register, so every mention of Heimdallr can be found in every text.

The word map carries all three for every word. A note made on the device can be cited exactly.

## 6. Typesetting

Desktop only. Rust.

- Shaping: cosmic-text (rustybuzz underneath).
- Line breaking: Knuth–Plass, with hyphenation patterns per language.
- Fonts: chosen per profile, shipped with the build, never substituted.
- Sizes: three to five presets per profile.
- Profiles: one per device.

| Profile | Screen | Notes |
|---|---|---|
| Elipsa 2E | 1404×1872, 227 ppi | greyscale |
| Libra Colour | 1264×1680, 300 ppi | colour layer for figures |

## 7. Artifact

Glyph runs. The desktop decides every glyph and where it goes. The device only draws them.

Per book, per profile, per size preset:

- **Fonts.** The exact font files, pinned by hash. A glyph ID means nothing without its font.
- **Pages.** For each page, lines of glyph runs: font, size, then each glyph's ID and position. Plus a few primitives: lines and rectangles for rules, borders and ornaments. Plus figures, placed by box.
- **Word map.** Every word: its text, its glyphs, its boxes, its block ID and offset, its canonical reference, its source page, its names.
- **Links.** Notes, cross-references and contents, as boxes with targets.
- **Notes.** Each note set on its own, for the note panel.

Notes don't take room from the page. Tap a marker and its note opens in a panel over the lower 40% of the screen. Tap outside it and it closes. A long note pages inside the panel. The page under it never changes, so citations and ink stay put.

A marker is a few pixels wide. Its tap box is at least 6 mm. When boxes overlap, the nearest marker wins.

Each note records the page its marker is on and the reference of that line (`inf.1.2`). The notes are also gathered at the end of the chapter, to read straight through. There each label links back to its marker's page.

Commentary on a passage (`::: commentary`) opens the same way, from a small ring in the margin beside the passage, left of any stanza number. Its title names the passage: `Stanza 5`, `Stanzas 5–6`, or a prose passage's first words.
- **Alignment.** References to the original, where the book has one.

Positions are fixed on the desktop, so the device can't drift from it. Rendering on the device only decides how each glyph's pixels look: anti-aliasing tuned to the panel.

Size: about 6–8 bytes a glyph and 2,000 glyphs a page. A long book in five sizes is about 80 MB raw, 15–25 MB compressed. Page images would be 300–600 MB.

Figures stay images: 4-bit greyscale on the Elipsa, colour on the Libra Colour.

A desktop tool renders any compiled page to PNG with the same code as the device. It is the preview, and the test that the device draws what the desktop meant.

## 8. Annotations

The device appends one record per stroke. It never edits or deletes earlier records. An erasure is a new record.

```json
{"id":"s_20261006T141502_412","book":"odyssey","profile":"elipsa-2e","size":2,
 "page":118,"tool":"pen","color":"black","width":3,
 "points":[[412,880],[415,881]],"time":1790966503}
```

Kollate resolves strokes against the word map: which words, which block, which reference. Anchors for the long term are block ID plus a quote: the exact words, with a little text before and after (the W3C TextQuoteSelector). They survive font changes, rebuilds and text fixes.

## 9. Device

- Draws pages from glyph runs, with a glyph cache. Turns pages.
- Glyph rendering: swash in Rust, or FreeType if built on KOReader's base.
- Pen: raw input, drawn with the fast waveform from its own loop. Target: under 5 ms per point, no stalls.
- Plumbing (suspend, frontlight, Wi-Fi, USB hand-off to Nickel) is borrowed, not written. Candidates: KOReader's device layer, Cadmus, FBInk.
- **Launched from NickelMenu, alongside Nickel.** A menu entry starts the reader. Nickel is paused or stopped while it runs, and restarted when it exits, the way KOReader and Plato do it.
- On exit, crash or USB connection, Nickel comes back. A crash must never leave the device stuck on a frozen screen.
- No changes to the firmware or to Nickel's files. Installing is copying a folder and a NickelMenu config line; removing is deleting them.

## 10. Kollate

Kollate reads the annotation files and the word map. It transcribes handwriting as it does now.

Open: Kollate never writes to the Kobo. Something must put compiled books on it. Either that rule changes, or a separate tool does it, or the user copies files by hand.

## 11. Open questions

- Colour figures on the Libra Colour: dither on the desktop, or leave to the device?
- Books with mixed verse and prose in one paragraph.
- Search on the device: from the word map, or not at all in v1.
- Text selection without a pen.
- Scripts beyond Latin: polytonic Greek is fonts; Hebrew runs right to left, which is real work for the typesetter.
- Word lookup in the original: tap a word, see its dictionary form and meaning. The first thing students of originals will ask for.
- Textual variants (TEI's apparatus criticus), where manuscripts disagree. For scholars. Later.

## 12. Milestones

Each ends in a go or no-go.

1. **Converter.** Three hard books into source, validated. Go: nothing lost that a reader would miss.
2. **Typeset chapter.** One chapter, one profile, one size, as glyph runs, previewed as PNG. Go: you would read it.
3. **Parallel text.** Gummere's *Beowulf* and the Old English, lines 1–100, aligned. Go: every line finds its original.
4. **Device viewer.** Glyph runs drawn on the Elipsa, from NickelMenu. Go: pages match the desktop preview, and page turns feel as good as KOReader.
5. **Pen.** Strokes recorded and resolved by Kollate. Go: under 5 ms per point, no stalls, words exact.
