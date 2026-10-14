# Wordhord: spec

Draft 2, 2026-10-07. Draft 1, a reader for old texts only, is in git history.

> A quiet e-reader for Kobo. Your books, your ink, your words, all in plain files on your device. Write in the margins with a pen. Tap any word for the whole of Wiktionary. Keep the words you meet.

*Wordhord* is Old English for "word-hoard", a store of words. In *Beowulf* (line 259) the leader of the Geats, asked who he is, *wordhord onleac*: unlocked his word-hoard. The vocab builder is the namesake.

## 1. Goal

A normal e-reader for stylus Kobos. Smaller and faster than KOReader. Built around three things the others do badly:

1. **The pen.** Ink in the margins, on any book. Strokes are stored so any tool can read them.
2. **Words.** Tap a word, even an inflected one, and get its full Wiktionary entry, offline. Keep it with the sentence you found it in. Review it later.
3. **Your data.** No account, no cloud, no telemetry. Everything Wordhord knows is a file you can open, copy, diff and delete.

**Target:** stylus Kobos (Elipsa 2E, Libra Colour), stock firmware. The app folder carries a profile for each, and Wordhord takes the one for the screen it finds; the Libra Colour's page-turn buttons turn pages. Kobos without a pen work too; they just lack ink. Launched from NickelMenu. Nickel pauses while Wordhord runs and resumes when it exits. Nickel stays in charge of the store, Wi-Fi setup, updates and USB.

## 2. Principles

- **Local first.** The device is the source of truth. Nothing needs a network. Sync is copying files, by whatever means you like.
- **Plain files, documented formats.** If Wordhord vanished tomorrow, your notes, ink and words would still be readable. Every format is in this spec.
- **Append, don't overwrite.** Notes, ink and reviews are logs. A deletion is a new record. History is never lost, and two copies merge.
- **Small on purpose.** Each feature earns its place. No plugins, no scripting, no settings for settings.
- **Fast where it shows.** Opening a book, turning a page, putting down ink. Budgets in §12.
- **Record, don't infer.** Anchors store the exact text they point at. Ink stores the raw points. Nothing is reconstructed later from guesses.

## 3. What it does

**The library** is searched from *search*, at the right of the shelves' heading on its home: titles, authors, series, tags and language by name, each word typed the start of one of theirs. Every search's line of text has *clear* at its end.

**Reading.** EPUB 2 and 3, reflowable, DRM-free. Type set on one screen, against a specimen passage with italics, figures and Old English and Norse letters: five faces, each marked with the languages it can set, by philologists' short names in small caps: *eur* (European: the Latin alphabet with Western and Central Europe's accents), *lat* (Latin, long vowels marked), *gk* (Greek, with accents and breathings), *oe*, *on*; *key*, at the right of the heading, opens what they stand for above the faces, and closes it; sizes by the printers' names (long primer to great primer); the measure in characters to a line (a measure no narrower than the page already is, as on the Libra's smaller screen in larger type, is set in grey: kept, but changing nothing at that size); leading; justified or ragged. Kept in `wordhord.toml`. Footnotes open in a panel at the foot of the page. A table of contents. Search within the book, from the contents: typed on Wordhord's own keyboard, set like the rest, with a row of one old language's letters over the alphabet, chosen by a key beside *123* that steps through them and is remembered while Wordhord runs: *oe* (þ ð æ ƿ ȝ and the long vowels), *on* (þ ð æ ǫ ø œ and the accented vowels), *lat* (the long vowels, æ, œ), *gk* (the accented vowels, and the alphabet below in the Greek layout); a row longer than ten has narrower keys. Words match loosely (no case, no accents, ǫ as o) and the last may be the start of one. Results by chapter, the words in their context in italic; a tap opens the page with them marked grey. Page turns by swipe or edge tap. The page itself carries only its folio. The bar at its top (a tap along the top) says where it is: the part of the book, the pages left in its chapter and how far through the book, in per cent, with the time and the battery (a figure and a filling hairline) between its controls, and the title on a line of its own. A tap in the top right corner hangs a ribbon from the top edge into the margin, forked as a hardback's silk; the contents list the ribbons under *Bookmarks*, each by its part and first words (and, once the book has underlines, offer *In the commonplace book*, its page there, whose passages take you to them), and they're kept in `notes.jsonl` by the page's first word. After any jump, to a chapter, through a link, to a search result, a ribbon or a passage, the foot of the page says *back to Book II, page 23 · stay*, centred in the folio's place: the first goes back, *stay* puts it away, and it fades by itself after ten pages read on. The light by a slide up or down, on a page or in the library: the left half for how bright, the right for how warm, with a hairline gauge at that edge until the next touch.

**Library.** Every EPUB on the device, found by a scan. Sorted by last read, title or author. Shelves are folders. Covers in a grid or a plain list. Nothing is imported or copied: the file on disk is the book.

**Pen.** Write anywhere on the page. Ink is drawn as you write, with the fast waveform. The pen only writes (§6).

**Highlights and notes.** A highlight is an underline: a fine rule under the words, as a pencil leaves in a printed book. Hold a word a second: it turns grey, and a loupe above the finger shows the word under it, set large, since the finger hides it. Drag over the passage, the loupe following with the word it ends on, and lift. Dragged on past the last line into the foot, the page turns and the passage carries on over it; and an underline that meets another across a page joins it, as within one. A hold that doesn't move still looks the word up. Tap reads, hold marks: a tap on an underlined word looks it up as any other; a hold on one greys the whole underline and shows its card: its note in italic, if it has one, over *note* (or *edit note*) *· write · remove underline · look up*. *Write* opens ruled lines under the passage, for a note in the pen's own hand: each stroke is kept as it's lifted, marked as the underline's, the eraser end rubs strokes out and *clear* all of them. The card then shows the writing, made to fit. It's a link made on purpose, not guessed from where ink lies; ink in the margin stays the margin's. A note is typed on ruled lines that wrap; an underline with one has a sideline, a fine rule in the left margin beside it; the commonplace book sets it under its passage after a pilcrow, and its search finds it; writing on an underline is drawn there too, under the passage and any typed note. An underline taken away takes its writing with it; underlines that merge stack theirs, in the order of their passages. Notes are records of their own in `notes.jsonl`, the latest for an underline standing, an empty one taking it away; underlines that merge keep their notes. The keyboard has a layer of figures and stops (*123*, and *abc* back), the old language's row over both. Underlines are anchored by paragraph and word, not page, so they keep to their words through any change of type, and keep their text in the log. Underline with the pen too, later. Add a written note with the pen, or a typed one.

**Dictionary.** Tap a word. Its Wiktionary entry opens in the panel: dictionary form, the grammar of the form you tapped, senses, etymology. A panel of more than one page says so at its foot, *1 of 2*; a tap on its right half, a swipe or a slide up pages on, and back the other way. Only a tap outside closes it: a finger that lingers or wanders in it, or the pen coming near, doesn't. Works on inflected forms (§7).

**The hoard.** One tap hoards a word, with its sentence, book and place. Unlock it on the device: review. Kollate takes it to Anki (§8, §11).

**Commonplace book.** Every highlight, note and kept word, across all books, in one place on the device. Kollate makes the Markdown. First, underlines: a row on the library's home opens an index of the books underlined in, the last first, each with its author and how many passages. A book's page sets its passages as quotations, each with the part of the book it's in and the day, *Book I · 8 October*. Handwriting is there too, among the passages in reading order: the strokes written on one page within two minutes of each other make one note, never two pages' strokes over each other, drawn to the page's scale under the line it was written by (*by “Expell'd and exil'd, left the Trojan shore.”*), with its part and day. A tap on it opens the book at that line. A tap on a passage opens the book at it. The index starts with *Search the commonplace book*: passages whose words, book or part start with each word typed, with the book named on each.

## 4. Not goals

- Store books. Kobo's DRM stays out. Read those in Nickel.
- PDFs and comics in v1. PDF with ink is the Elipsa's other big use. It's a later decision (§15).
- Cloud sync, accounts, social anything.
- Plugins and scripting.
- Every EPUB's design. Wordhord keeps a book's structure and emphasis, not every CSS rule. Text is set Wordhord's way.
- Translation, summaries, or any model on the device.

## 5. Reading engine

Wordhord lays out text on the device. The typesetter from draft 1 moves onto the Kobo: Knuth–Plass line breaking, hyphenation, real kerning and ligatures through rustybuzz, glyphs drawn with swash. It already runs there for drawing.

**EPUB to blocks.** Each chapter's XHTML becomes Wordhord's block model: headings, paragraphs, quotes, lists, verse, tables, figures, notes. CSS is read only for what carries meaning: italics, bold, small caps, alignment, indents, page breaks, hidden text. Text hidden for the eye but kept for screen readers, moved far off the page (`left: -999em`) or clipped to nothing, is hidden here too: a title page's title, under its drawing of it. The rest is dropped. Draft 1's importer does most of this already, one book at a time, by recipe. Here it must work without a recipe, on any reasonable EPUB.

**Our own engine.** Decided 2026-10-07: no Plato, no crengine. Wordhord stays GPL-3.0, and its pages are set the way its typesetter sets them.

**No desktop step for books.** Copy an EPUB to the Kobo and open it. The desktop builds only dictionary packs and, if you want them, editions (§14).

**Pages are computed once and cached.** A chapter is laid out the first time it's opened, then stored as glyph runs, the format draft 1 uses. The cache is keyed by book hash, chapter and settings. Change the font size and the cache rebuilds in the background, chapter by chapter, nearest first.

**Layout is deterministic.** The same book and settings give the same pages, byte for byte. Page numbers mean something and stay put.

**Footnotes** open in the panel. EPUB 3's `epub:type="noteref"` is used where present. Otherwise, links into a notes section are recognised by shape.

**Images** are dithered to 16 greys on the Elipsa and drawn in colour on the Libra Colour. SVG too, as title pages and publishers' marks often are (Standard Ebooks' all are): drawn by resvg, paths only, with no text or fonts of its own.

## 6. Pen

The Kobo Stylus 2 has a tip, a button and an eraser end. Wordhord reads the pen's input device directly, draws each point from its own loop with the fast waveform, and records every point.

**No gestures.** The pen only writes. A circle meant as ink must never open the dictionary, nor a tick hoard a word: guessing what a stroke means is what Wordhord exists not to do. The stylus's own controls do the rest: the eraser end erases, and a stroke with the button held underlines. Kollate can read circles and ticks on the desktop, where a wrong guess costs nothing.

**Ink belongs to the text, not the page.** Each stroke is anchored to the words nearest it, with its offset from them. Change the font size and a note in the margin moves with its paragraph. Ink across the text, like an underline, stays with its words. Ink that can't be anchored, on a blank page or an image, keeps its place for that layout. A stroke is measured from its word in that word's line heights, so it grows with the type; one in a margin starts at the same share of the margin's width and keeps its shape from there. The pen only writes, and only on a book's pages: a touch of its tip is ink, a full stop as much as a line. Fingers do the rest, and are ignored while the pen is near the screen and for a second after, so the hand that writes can rest on it; meanwhile a manicule, ☛, the hand readers drew in their margins, points in at the page from the top left (the top right is the ribbon's), and fingers count again the moment it goes. An underline that overlaps or touches others takes them into it. The stylus's other end rubs out ink and underlines it passes over; a stroke with its button held underlines the words it runs under.

**Strokes are data.** Raw points with pressure and time, as recorded, in the log (§9). A stroke can be redrawn exactly, replayed, sent to a handwriting model or turned into SVG. Kollate already transcribes handwriting locally. It reads Wordhord's ink the same way.

## 7. Dictionary

**Source:** Wiktionary, through kaikki.org's structured extracts. CC BY-SA, credited on every entry.

**One pack per language.** A SQLite file built on the desktop from kaikki's data: headwords, senses, etymology, pronunciation, and every inflected form with its grammar. Old Norse and Old English packs are a few megabytes. English is gigabytes raw and must be trimmed (§15). Packs go in `dictionaries/` and are found by language code. Ancient Greek (`grc`) and Latin (`lat`) too: the loose match takes off accents, breathings and long marks, and a Latin word with *-que* or *-ne* joined to it is looked up without, if nothing knew it whole. Greek and Latin books are hyphenated by TeX's own patterns for them (`hyph-grc`, `hyph-la-x-classic`), carried in the typesetter.

**The packs.** Five for reading (English, Latin, Ancient Greek, Old English, Old Norse) and fifteen for following etymologies: Middle English, Old French, Middle French, Old High German, Old Saxon, Old Frisian, Middle Dutch, Gothic, Old Irish, Old Church Slavonic, Sanskrit, and the reconstructed Proto-Germanic, Proto-West Germanic, Proto-Italic and Proto-Indo-European. `scripts/build-dictionaries.sh` builds them all from kaikki.org; `scripts/release-dictionaries.sh` zips each with its licence note, for a release, `dictionaries-YYYY-MM`. A word tapped in a book falls through from its book's language to the others, but never to a reconstructed language: no book is written in one.

**Inflected forms.** The pack maps each form to its dictionary form and grammar, from Wiktionary's own tables. Tap *mǫgu* and get "accusative plural of *mǫgr*: son, boy, youth". Tap *went* and get "past of *go*". Nickel and KOReader look up the spelling you tapped, and often miss. Where a form has several readings, all are shown.

**The book's language picks the pack.** It comes from the EPUB's `dc:language`, or from an element's `lang`, so a Latin phrase in an English book opens the Latin pack.

**Other scripts.** Etymologies cite words in scripts no face has. After DejaVu, a profile lists fonts for them, tried in order: Noto Serif Hebrew, Noto Naskh Arabic and Noto Serif Devanagari, so Hebrew *לִוְיָתָן*, Arabic and Sanskrit *वीर* show as themselves. A word in a script written right to left is shaped so; a phrase of several such words would want the bidirectional algorithm, which isn't here yet. Chinese, Japanese and Korean, in some eight thousand English entries, aren't covered: their fonts are some twenty megabytes.

**Etymology trail.** In an entry's etymology, an ancestor in a language with a pack installed is set in italic: the word straight after the language's name, *hate* → Old English *hatian*, Old Norse *hatr*. A reconstruction, *\*hataz*, links into a reconstructed language's pack, its star left outside the italic; and not after a longer name: Middle English isn't English. Late, Medieval and New Latin are Latin, as Wiktionary files them. A word is a link only if its pack has it, so no tap leads nowhere. A followed word keeps the etymology's spelling: a laryngeal *H* stays capital. A hairline under it says it can be followed. A tap opens its entry from its own pack, in the panel, with *‹ hate* at the top to go back; its etymology may go on. An ancestor isn't hoarded from there: the sentence it would be kept with isn't its.

**Corrections.** A small file per pack, `corrections.toml`, fixes or adds entries. Yours, versioned, merged when the pack is built.

## 8. The hoard

The vocab builder is *the hoard*. You *hoard* a word; reviewing is *unlocking* it, after *wordhord onleac*.

**Hoarding a word** records the word as written, its dictionary form, the sense you were shown, the sentence, the book, the anchor and the time. From the *hoard* mark at the end of each reading's headline in the dictionary panel. One reading is kept for each place a word is met: another reading's mark moves it there, and the kept reading's own mark, *in the hoard*, lets it go.

**Unlocking, on the device.** *Unlock the hoard* on the library's home, with how many words are due. One card at a time. The front shows a sentence the word was kept from, the word in italic; another of its sentences each time, if it has several. A tap shows the entry: the word's own readings only. Then four grades: *again*, *hard*, *good*, *easy*, each saying when the word comes back. *Again* brings it round once more before the end. At most ten new words a day. Scheduled with FSRS 4.5 and its default weights, at nine in ten recalled. Reviews are appended to the log, never edited, so the schedule is always recomputed from history.

**The hoard.** A screen with every word you've kept: by book, by language, by date, or by how well you know it. Tap one to see every sentence you met it in.

**No export on the device.** Kollate already builds Anki decks from Kobo vocab, with the sentence as the cue, and Markdown from notes. It reads Wordhord's the same way. Wordhord keeps the logs; Kollate turns them into other things.

## 9. Data on the device

Everything lives in one folder, `/mnt/onboard/.wordhord/`. It shows over USB like any other files.

```
.wordhord/
  wordhord.toml          settings
  library.sqlite         books found, positions, last read: a cache, rebuilt by a scan
  books/<book hash>/
    notes.jsonl          highlights, notes, ink, bookmarks: the log
    pages/<settings>/    cached layout: safe to delete, rebuilt on demand
  hoard.jsonl            hoarded words and reviews: the log
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

**Export** is Kollate's: Markdown, ink as SVG, Anki. The logs are plain JSON Lines, so any other tool can read them too.

## 10. Local-first sync

Wordhord has no server and wants none. Ways to move your data, all optional:

- **USB.** Plug in and copy `.wordhord/`. Kollate does this, read-only, as it does today.
- **A folder over Wi-Fi.** Point Wordhord at a WebDAV folder on your own machine or NAS. It pushes its logs, pulls the others, and merges by ID. Two Kobos share notes and vocab this way.
- **Syncthing**, if you run it, on the same folder.

Books come by USB, by that same folder, or from an OPDS catalogue such as calibre's content server.

## 11. Kollate

Kollate is the desktop companion. It reads `.wordhord/` over USB and never writes to it. It shows notes and ink beside the text, transcribes handwriting locally, and builds Anki decks. Today it reads Nickel's database and KOReader's sidecars. Wordhord's logs become a third source, and the simplest. In progress.

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

**Kobo** (first). FBInk draws and refreshes. Input devices are read raw. NickelMenu starts a script that pauses Nickel with SIGSTOP and saves its screen; both come back on exit or crash. Wordhord closes itself after 30 minutes untouched, so Nickel's sleep and power handling return. Nickel holds the power button and the sleep cover, so Wordhord can't grab them; a power press while Wordhord runs reaches Nickel when it resumes. Wordhord closes from the foot of the library's home. It sleeps on its own: Nickel holds the power button's events, but Wordhord asks every 50 ms whether the button is down (EVIOCGKEY), and a press, or fifteen minutes untouched, shows the sleep screen, puts the light out and suspends the Kobo as KOReader does (state-extended, a pause, sync, wakeup_count, then `mem`). The e-ink controller refuses to suspend while it still powers the panel after drawing ("xon off timer pending"), so a refusal is tried again each second. On the charger a MediaTek Kobo crashes if it suspends, and with Wi-Fi left on by Nickel it may, so in either case the sleep screen waits for the button without suspending. Waking puts the screen back as it was. The presses also queue for Nickel, which sleeps and wakes once on its return. The sleep screen is Knud Baade's *Fantasibilde fra den norske sagatid* (1850): the warden on the sea-cliff, the coast-guard who challenges Beowulf before he *wordhord onleac*. With Nickel paused, Wordhord sets the front light itself, through the sysfs files KOReader uses. It starts from Nickel's own settings (the Libra Colour's brightness file reads 0, lit or not), and on exit puts Nickel's level back, since Nickel can't be told a new one while it runs; a warmth Nickel sets by the hour is left to it.

**PineNote** (later). The same 10.3" 1404×1872 panel as the Elipsa 2E, so layouts and cached pages carry over unchanged. It runs ordinary Linux, so Wordhord is a normal program there: no host app to pause. The pen is a Wacom digitiser, a standard input device. The work is the display: the `rockchip_ebc` e-ink driver, which is still changing. Check its state before starting.

**Recording**, for demonstrations. With a file named `record` in the app folder, each session is recorded to `recordings/`: every draw with its pixels, every gesture, every point of the pen, each with its time. The pixels are deflated on a thread of their own, so the recording doesn't slow what it records. `scripts/replay.py` makes a video of it at the speed it happened, with a soft dot where a finger touched and a fine ring where the pen wrote, long pauses cut short and a full refresh's flash shown. On the desktop, `WORDHORD_PACE` gives a script a person's pace, for recording one.

**Desktop** (for development). A window that behaves like the device: the same pixels, mouse clicks as taps, drags as swipes, a tablet as the pen. Most testing happens here, without copying anything to a Kobo. Waveforms are simulated, slowly, so a missed refresh shows.

## 14. Spice

Small things that make it Wordhord's:

- **The hoard grows.** The home screen's one number is how many words you've kept. No streaks, no nagging.
- **Words met before.** A faint dotted line under words you've kept and not yet learned, wherever they turn up again, in any book, in any of their forms as the part of speech kept (*hate* the noun: hate, hates, hate's): every form of each, from its pack, gathered once and again when the hoard changes, so a page only asks a set. Not where an underline is. Learned is FSRS's stability at three weeks or more, as Anki calls a card mature: a word you know needs no mark, and one forgotten (*again*) has it back. No switch: the mark goes where it's no longer needed. Tapped, the entry starts *Met before, in* the book, with the words around where it was first kept; not where it was kept, which says *in the hoard*. Packs index their forms by entry for this.
- **Etymology trail** (§7). Tap your way to Proto-Indo-European.
- **Margin index.** One screen with every page you wrote on, as thumbnails of the ink. Find a note by its shape.
- **Reading ledger.** Time read, pages turned, words kept. Local, private, exportable.
- **Editions.** Books in Wordhord's own source format (Djot with a TEI vocabulary, from draft 1, see VOCABULARY.md) still compile on the desktop into fine editions: verse set as verse, line numbers, commentary, and each stanza's translation in the panel. The *Völuspá* reads that way today.

## 15. Open decisions

- **PDF with ink.** MuPDF is the obvious engine (AGPL). PDFs are where Elipsa owners write most. After v1, or in it?
- **English dictionary size.** Trim to senses and forms, or ship it whole?
- **Exit and sleep.** Settled: Wordhord sleeps itself, and closes from home.
- **Searching ink.** That needs transcription. Kollate does it on the desktop today. Is that enough?

## 16. Milestones

Each ends in a go or no-go.

1. **Any EPUB, one chapter.** Laid out on the desktop backend, then on the Kobo. Test set: *Children of Ash and Elm* (Basic Books), *The Prose Edda* and Michael Alexander's *Beowulf* (Penguin), Emily Wilson's *Odyssey* (Norton), Davidson's *Gods and Myths of Northern Europe* (Penguin), Crawford's *Poetic Edda* (Hackett, with image drop caps), *The Hammer and the Cross* (Tor), *Learning Go* (O'Reilly), and Standard Ebooks' *Moby-Dick* and *Canterbury Tales* as controls. Go: you'd read them, and page turns meet budget.
2. **Library and position.** Scan, list, open, resume. Go: launch to last page in under a second.
3. **Dictionary.** English and Old Norse packs; tap a word, see the entry, inflected forms included. Go: four taps in five find the right entry.
4. **Pen.** Ink, anchors, the log. Go: under 5 ms per point, and ink follows its paragraph through a font change.
5. **The hoard.** Hoard and unlock. Go: you use it for a week.
6. **Kollate reads it all** (in progress). Notes, ink and words on the desktop, and on to Anki. Go: a round trip with nothing lost.
