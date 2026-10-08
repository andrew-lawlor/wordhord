# Wordhord: spec

Draft 2, 2026-10-07. Draft 1, a reader for old texts only, is in git history.

> A quiet e-reader for Kobo. Your books, your ink, your words, all in plain files on your device. Write in the margins with a pen. Tap any word for the whole of Wiktionary. Keep the words you meet.

*Wordhord* is Old English for "word-hoard", a store of words. In *Beowulf* (line 259) the leader of the Geats, asked who he is, *wordhord onleac*: unlocked his word-hoard. The vocab builder is the namesake.

## 1. Goal

A normal e-reader for stylus Kobos. Smaller and faster than KOReader. Built around three things the others do badly:

1. **The pen.** Ink in the margins, on any book. Strokes are stored so any tool can read them.
2. **Words.** Tap a word, even an inflected one, and get its full Wiktionary entry, offline. Keep it with the sentence you found it in. Review it later.
3. **Your data.** No account, no cloud, no telemetry. Everything Wordhord knows is a file you can open, copy, diff and delete.

**Target:** stylus Kobos (Elipsa 2E, Libra Colour), stock firmware. Kobos without a pen work too; they just lack ink. Launched from NickelMenu. Nickel pauses while Wordhord runs and resumes when it exits. Nickel stays in charge of the store, Wi-Fi setup, updates and USB.

## 2. Principles

- **Local first.** The device is the source of truth. Nothing needs a network. Sync is copying files, by whatever means you like.
- **Plain files, documented formats.** If Wordhord vanished tomorrow, your notes, ink and words would still be readable. Every format is in this spec.
- **Append, don't overwrite.** Notes, ink and reviews are logs. A deletion is a new record. History is never lost, and two copies merge.
- **Small on purpose.** Each feature earns its place. No plugins, no scripting, no settings for settings.
- **Fast where it shows.** Opening a book, turning a page, putting down ink. Budgets in §12.
- **Record, don't infer.** Anchors store the exact text they point at. Ink stores the raw points. Nothing is reconstructed later from guesses.

## 3. What it does

**Reading.** EPUB 2 and 3, reflowable, DRM-free. Font, size, margins, line spacing. Footnotes open in a panel at the foot of the page. A table of contents. Search within the book. Page turns by swipe or edge tap.

**Library.** Every EPUB on the device, found by a scan. Sorted by last read, title or author. Shelves are folders. Covers in a grid or a plain list. Nothing is imported or copied: the file on disk is the book.

**Pen.** Write anywhere on the page. Ink is drawn as you write, with the fast waveform. Gestures in §6.

**Highlights and notes.** Underline with the pen, or press and drag with a finger. Add a typed or a written note.

**Dictionary.** Tap a word. Its Wiktionary entry opens in the panel: dictionary form, the grammar of the form you tapped, senses, etymology. Works on inflected forms (§7).

**Vocab.** One tap keeps a word, with its sentence, book and place. Review on the device. Export anywhere (§8).

**Commonplace book.** Every highlight, note and kept word, across all books, in one place on the device and in one Markdown file on export.

## 4. Not goals

- Store books. Kobo's DRM stays out. Read those in Nickel.
- PDFs and comics in v1. PDF with ink is the Elipsa's other big use. It's a later decision (§15).
- Cloud sync, accounts, social anything.
- Plugins and scripting.
- Every EPUB's design. Wordhord keeps a book's structure and emphasis, not every CSS rule. Text is set Wordhord's way.
- Translation, summaries, or any model on the device.

## 5. Reading engine

Wordhord lays out text on the device. The typesetter from draft 1 moves onto the Kobo: Knuth–Plass line breaking, hyphenation, real kerning and ligatures through rustybuzz, glyphs drawn with swash. It already runs there for drawing.

**EPUB to blocks.** Each chapter's XHTML becomes Wordhord's block model: headings, paragraphs, quotes, lists, verse, tables, figures, notes. CSS is read only for what carries meaning: italics, bold, small caps, alignment, indents, page breaks, hidden text. The rest is dropped. Draft 1's importer does most of this already, one book at a time, by recipe. Here it must work without a recipe, on any reasonable EPUB.

**Our own engine.** Decided 2026-10-07: no Plato, no crengine. Wordhord stays GPL-3.0, and its pages are set the way its typesetter sets them.

**No desktop step for books.** Copy an EPUB to the Kobo and open it. The desktop builds only dictionary packs and, if you want them, editions (§14).

**Pages are computed once and cached.** A chapter is laid out the first time it's opened, then stored as glyph runs, the format draft 1 uses. The cache is keyed by book hash, chapter and settings. Change the font size and the cache rebuilds in the background, chapter by chapter, nearest first.

**Layout is deterministic.** The same book and settings give the same pages, byte for byte. Page numbers mean something and stay put.

**Footnotes** open in the panel. EPUB 3's `epub:type="noteref"` is used where present. Otherwise, links into a notes section are recognised by shape.

**Images** are dithered to 16 greys on the Elipsa and drawn in colour on the Libra Colour.

## 6. Pen

The Kobo Stylus 2 has a tip, a button and an eraser end. Wordhord reads the pen's input device directly, draws each point from its own loop with the fast waveform, and records every point.

**Gestures**, written with the pen:

| Stroke | Means |
|---|---|
| A line under words | Highlight them |
| A circle round one word | Look it up |
| A circle, then a tick | Keep it as vocab |
| A scribble over ink | Erase that ink |
| Anything else | Ink, as drawn |

Hold the button while writing: always ink, never a gesture. The eraser end: erase.

**Ink belongs to the text, not the page.** Each stroke is anchored to the words nearest it, with its offset from them. Change the font size and a note in the margin moves with its paragraph. Ink across the text, like an underline, stays with its words. Ink that can't be anchored, on a blank page or an image, keeps its place for that layout.

**Strokes are data.** Raw points with pressure and time, as recorded, in the log (§9). A stroke can be redrawn exactly, replayed, sent to a handwriting model or turned into SVG. Kollate already transcribes handwriting locally. It reads Wordhord's ink the same way.

## 7. Dictionary

**Source:** Wiktionary, through kaikki.org's structured extracts. CC BY-SA, credited on every entry.

**One pack per language.** A SQLite file built on the desktop from kaikki's data: headwords, senses, etymology, pronunciation, and every inflected form with its grammar. Old Norse and Old English packs are a few megabytes. English is gigabytes raw and must be trimmed (§15). Packs go in `dictionaries/` and are found by language code.

**Inflected forms.** The pack maps each form to its dictionary form and grammar, from Wiktionary's own tables. Tap *mǫgu* and get "accusative plural of *mǫgr*: son, boy, youth". Tap *went* and get "past of *go*". Nickel and KOReader look up the spelling you tapped, and often miss. Where a form has several readings, all are shown.

**The book's language picks the pack.** It comes from the EPUB's `dc:language`, or from an element's `lang`, so a Latin phrase in an English book opens the Latin pack.

**Etymology trail.** In an entry, ancestor words are links. *word* → Old English *word* → Proto-Germanic *\*wurdą* → Proto-Indo-European *\*wr̥dʰh₁om*. Each opens in the panel, if its language's pack is installed. Tap your way back four thousand years.

**Corrections.** A small file per pack, `corrections.toml`, fixes or adds entries. Yours, versioned, merged when the pack is built.

## 8. Vocab

**Keeping a word** records the word as written, its dictionary form, the sense you were shown, the sentence, the book, the anchor and the time. From a tap in the dictionary panel, or the pen's circle and tick.

**Review on the device.** Short sessions, one card at a time. The front shows the sentence with the word marked. The back shows the entry. Scheduled with FSRS, the open spaced-repetition algorithm Anki offers. Reviews are appended to the log, never edited, so the schedule can always be recomputed from history.

**Your hoard.** A screen with every word you've kept: by book, by language, by date, or by how well you know it. Tap one to see every sentence you met it in.

**Export.** Anki, CSV and Markdown. Kollate already builds Anki decks from Kobo vocab, with the sentence as the cue. It reads Wordhord's the same way.

## 9. Data on the device

Everything lives in one folder, `/mnt/onboard/.wordhord/`. It shows over USB like any other files.

```
.wordhord/
  wordhord.toml          settings
  library.sqlite         books found, positions, last read: a cache, rebuilt by a scan
  books/<book hash>/
    notes.jsonl          highlights, notes, ink, bookmarks: the log
    pages/<settings>/    cached layout: safe to delete, rebuilt on demand
  vocab.jsonl            kept words and reviews: the log
  dictionaries/<lang>.sqlite
  log/                   what went wrong, kept small
```

**Books are known by content hash.** Rename the file, move it, download it again: its notes follow, because the hash is the same. A different edition gets a new hash, and its notes are re-anchored by quote (below).

**Logs are JSON Lines.** One record per line, append only, each with an ID and a time. Two copies of a log merge by taking every ID from both. That's all sync needs.

```json
{"id":"01J9Z2Q3","t":"2026-10-07T14:15:02Z","kind":"ink","book":"sha256:9f2c…",
 "anchor":{"chapter":"ch04.xhtml","quote":"the wine-dark sea","prefix":"over ","suffix":", to"},
 "offset":[-412,18],"tool":"pen","width":2,
 "points":[[0,0,0.41,0],[3,1,0.47,8],[7,1,0.52,15]]}
```

Points are `[x, y, pressure, ms]`, relative to the anchor's first word.

**Anchors** follow the W3C Web Annotation model: the exact text quoted, with a little before and after. They survive re-layout, a new edition and fixed typos. A chapter path and character offset make lookup fast; the quote makes it safe.

**Export** writes, for each book, Markdown (quotes, notes, and ink as SVG beside them) and W3C Web Annotation JSON-LD for tools that speak it. One more Markdown file holds the whole commonplace book.

## 10. Local-first sync

Wordhord has no server and wants none. Ways to move your data, all optional:

- **USB.** Plug in and copy `.wordhord/`. Kollate does this, read-only, as it does today.
- **A folder over Wi-Fi.** Point Wordhord at a WebDAV folder on your own machine or NAS. It pushes its logs, pulls the others, and merges by ID. Two Kobos share notes and vocab this way.
- **Syncthing**, if you run it, on the same folder.

Books come by USB, by that same folder, or from an OPDS catalogue such as calibre's content server.

## 11. Kollate

Kollate is the desktop companion. It reads `.wordhord/` over USB and never writes to it. It shows notes and ink beside the text, transcribes handwriting locally, and builds Anki decks. Today it reads Nickel's database and KOReader's sidecars. Wordhord's logs become a third source, and the simplest.

## 12. Budgets

On the Elipsa 2E:

| What | Budget |
|---|---|
| Launch to last page | under 1 s |
| Page turn, before the e-ink refresh | under 50 ms |
| Ink, pen to pixel | under 5 ms per point, no stalls |
| Dictionary panel open | under 150 ms |
| First open of a 300-page book | first page under 2 s, the rest in the background |
| App size, without dictionaries | under 10 MB |

Measured on the device and written to the log, so regressions show.

## 13. Platforms

Everything that touches hardware sits behind one thin layer, so the rest of Wordhord never knows what it runs on:

- **Screen:** draw a region of grey pixels, refresh it with a waveform (full, fast, or ink).
- **Input:** finger, pen and button events, already turned the right way up.
- **Power:** sleep, wake, battery, frontlight.
- **Lifecycle:** start, hand back, crash safely.

Three backends:

**Kobo** (first). FBInk draws and refreshes. Input devices are read raw. NickelMenu starts a script that pauses Nickel with SIGSTOP and saves its screen; both come back on exit or crash. Wordhord closes itself after 30 minutes untouched, so Nickel's sleep and power handling return. Nickel holds the power button and the sleep cover, so Wordhord can't grab them; a power press while Wordhord runs reaches Nickel when it resumes. Wordhord closes from the foot of the library's home; it needs, later, its own sleep.

**PineNote** (later). The same 10.3" 1404×1872 panel as the Elipsa 2E, so layouts and cached pages carry over unchanged. It runs ordinary Linux, so Wordhord is a normal program there: no host app to pause. The pen is a Wacom digitiser, a standard input device. The work is the display: the `rockchip_ebc` e-ink driver, which is still changing. Check its state before starting.

**Desktop** (for development). A window that behaves like the device: the same pixels, mouse clicks as taps, drags as swipes, a tablet as the pen. Most testing happens here, without copying anything to a Kobo. Waveforms are simulated, slowly, so a missed refresh shows.

## 14. Spice

Small things that make it Wordhord's:

- **The hoard grows.** The home screen's one number is how many words you've kept. No streaks, no nagging.
- **Words met before.** A faint mark under words you've kept, wherever they turn up again, in any book. Tap one to see where you first met it.
- **Etymology trail** (§7). Tap your way to Proto-Indo-European.
- **Quote cards.** Any highlight, set as a clean page in the book's own type, exported as an image to share.
- **Margin index.** One screen with every page you wrote on, as thumbnails of the ink. Find a note by its shape.
- **Reading ledger.** Time read, pages turned, words kept. Local, private, exportable.
- **Editions.** Books in Wordhord's own source format (Djot with a TEI vocabulary, from draft 1, see VOCABULARY.md) still compile on the desktop into fine editions: verse set as verse, line numbers, commentary, and each stanza's translation in the panel. The *Völuspá* reads that way today.

## 15. Open decisions

- **PDF with ink.** MuPDF is the obvious engine (AGPL). PDFs are where Elipsa owners write most. After v1, or in it?
- **English dictionary size.** Trim to senses and forms, or ship it whole?
- **Exit and sleep.** A gesture to leave and Wordhord's own sleep screen, or always hand back to Nickel?
- **Searching ink.** That needs transcription. Kollate does it on the desktop today. Is that enough?

## 16. Milestones

Each ends in a go or no-go.

1. **Any EPUB, one chapter.** Laid out on the desktop backend, then on the Kobo. Test set: *Children of Ash and Elm* (Basic Books), *The Prose Edda* and Michael Alexander's *Beowulf* (Penguin), Emily Wilson's *Odyssey* (Norton), Davidson's *Gods and Myths of Northern Europe* (Penguin), Crawford's *Poetic Edda* (Hackett, with image drop caps), *The Hammer and the Cross* (Tor), *Learning Go* (O'Reilly), and Standard Ebooks' *Moby-Dick* and *Canterbury Tales* as controls. Go: you'd read them, and page turns meet budget.
2. **Library and position.** Scan, list, open, resume. Go: launch to last page in under a second.
3. **Dictionary.** English and Old Norse packs; tap a word, see the entry, inflected forms included. Go: four taps in five find the right entry.
4. **Pen.** Ink, gestures, anchors, the log. Go: under 5 ms per point, and ink follows its paragraph through a font change.
5. **Vocab.** Keep, review, export to Anki. Go: you use it for a week.
6. **Kollate reads it all.** Notes, ink and words on the desktop. Go: a round trip with nothing lost.
