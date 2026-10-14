# Wordhord

An e-reader for Kobo, made the way books were. Better still with a pen.

In *Beowulf*, a warden on the sea-cliffs asks the strangers who they are. Beowulf answers, and the poet says *wordhord onleac*: he unlocked his word-hoard. Wordhord is a reading app built around that idea. You read, you keep the words you meet, and you write in the margins.

It runs on your Kobo alongside Kobo's own reading app, and puts it back when you close it. It reads your EPUB books. It has no store, no account and no cloud. Everything stays on your Kobo.

<p>
<img src="docs/etymology-trail.gif" width="32%" alt="Moby-Dick's first page: 'spleen' is tapped; its history names Latin splēn, which opens, and then Ancient Greek σπλήν; back again, and the word is saved">
<img src="docs/pen-underline.gif" width="32%" alt="The pen draws a rough line under 'Circumambulate the city of a dreamy Sabbath afternoon', and it becomes a clean underline">
<img src="docs/page.png" width="32%" alt="A page of Dryden's Aeneid with two passages underlined, a word circled in ink, a tick in the margin and a ribbon bookmark">
</p>

## Reading

**Pages set like a printed book.** Wordhord lays out each paragraph as a whole, the way fine printing does, so the spaces between words stay even. Kobo's own app fills one line at a time, which can leave gaps. Verse keeps its lines. Numbers and small capitals look as they would in a good book.

**Type you choose by eye.** Pick a typeface, size and line spacing, and see a sample page change as you go. Each typeface is marked with the languages it can show.

**The rest of a reader.** Search a book or your library. Bookmark a page with a ribbon. Slide a finger up or down the page to change the light: the left side for brightness, the right for warmth. When the Kobo sleeps, it shows a painting of the warden on his cliff.

<p>
<img src="docs/type.png" width="32%" alt="The type screen: a sample passage, then the typefaces, each with the languages it can show, then size, line length, spacing and alignment">
<img src="docs/home.png" width="32%" alt="The library's home: books being read, the shelves, saved words, and the commonplace book">
<img src="docs/sleep.png" width="32%" alt="The sleep screen: Knud Baade's painting of a warden on a sea-cliff under a stormy moon, with WORDHORD over the sky">
</p>

## Words

**Tap any word to look it up.** The dictionary is Wiktionary, kept on your Kobo, so it works offline. It understands word forms: tap *went* and it finds *go*.

**Follow a word's history.** A dictionary entry tells where a word came from. Where that's another language Wordhord knows, the older word is underlined. Tap it to open it. *Spleen* leads to Latin, then to Greek. Latin *vir*, "man", leads back to Proto-Indo-European, about six thousand years.

**Keep the words you meet.** One tap saves a word with the sentence you found it in. Wordhord brings saved words back as flashcards, spaced out so you remember them, the way Anki does it.

**Meet them again.** A word you're still learning gets a faint dotted line wherever it turns up, in any book. Tap it, and it reminds you where you first met it. Once you know the word, the line goes.

**Old languages too.** Read Latin, Ancient Greek, Old English or Old Norse, and look words up in them. The on-screen keyboard has their letters.

<p>
<img src="docs/met-before.gif" width="32%" alt="'leviathan' is saved, then met again in a later chapter with a dotted line under it; tapped, its entry begins 'Met before, in Moby-Dick'">
<img src="docs/unlock-the-hoard.gif" width="32%" alt="Reviewing saved words: a card shows 'spleen' in its line from Moby-Dick, the definition is shown, and the card is graded">
<img src="docs/latin-coda.gif" width="32%" alt="Virgil's Latin: 'virumque' in the first line of the Aeneid is tapped, and its entry is vir, man, with the example 'Arma virumque cano'">
</p>

## Writing with the pen

For Kobos with a stylus, such as the Elipsa and the Libra Colour.

**The pen only writes.** A dot is a dot, never a page turn. Rest your hand on the screen while you write. A small pointing hand at the top of the page shows Wordhord knows the pen is near. Turn the stylus over to erase.

**Underline as you would on paper.** Hold the pen's button and draw under a line, and it snaps to a clean underline. No pen? Hold a word with your finger and drag.

**Notes on what you underline.** Hold an underline to add a note, typed or in your own hand. A mark in the margin shows which passages have one.

**Your writing stays with the words.** Make the text bigger or smaller, and your notes in the margin move with the lines they belong to.

<p>
<img src="docs/written-note.gif" width="32%" alt="An underline is held, write is chosen, and 'cf. Job 41.' is written by hand; the underline then shows it">
<img src="docs/detail.png" width="64%" alt="A close view of a page: an underline with a mark in the margin beside it, a word circled in ink, a tick in the margin, and faint dotted lines under saved words">
</p>

## Your notes

**A commonplace book.** Everything you underline gathers in one place, by book and by day, with your notes, typed or handwritten. Search it. Open it from your library or from a book's contents.

**Kept as plain files.** Your saved words, underlines, notes and handwriting are ordinary files on your Kobo, never sent anywhere, and nothing is ever overwritten. [Kollate](https://github.com/andrew-lawlor/kollate), a desktop app, will read them.

<p>
<img src="docs/commonplace.png" width="32%" alt="A book's page in the commonplace book: passages underlined, one with a typed note, one with a handwritten note">
</p>

## Which Kobos

Wordhord is early, and in daily use on a Kobo Elipsa 2E and a Kobo Libra Colour. A Kobo with the same screen, such as the Libra 2, should work but hasn't been tried. Other models need a small settings file for their screen first. Without a pen, you get everything but the handwriting.

It reads EPUB books without DRM. Books bought from the Kobo store are locked to Kobo's own app and stay there. Wordhord tells you how many it can't open, so you know they weren't lost.

## Getting started

You need [NickelMenu](https://pgaskin.net/NickelMenu/), a free add-on that puts a menu in Kobo's own app, and Wordhord built from its source:

```
git clone --recursive https://github.com/andrew-lawlor/wordhord
cd wordhord
scripts/package-kobo.sh
```

Building needs Rust with the `armv7-unknown-linux-musleabihf` target, [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) and zig. Plug in your Kobo, copy `dist/kobo/.adds/` into the `.adds` folder at its root, and eject it. *Wordhord* is now in NickelMenu.

Dictionaries are separate downloads, one a language, from the [dictionary release](https://github.com/andrew-lawlor/wordhord/releases/tag/dictionaries-2026-10). Unzip the ones you want into `.adds/wordhord/dictionaries/` on your Kobo. English is the largest, at about 300 MB. Most of the old languages are a few megabytes.

## How it works

Wordhord is written in Rust. NickelMenu starts it. It pauses Nickel, Kobo's reading app, saves the screen, and restores both when it closes. It draws with [FBInk](https://github.com/NiLuJe/FBInk) and reads the touch screen and pen directly, so palm rejection, the pen's buttons and the eraser are its own.

It opens EPUBs with its own engine, which keeps only the styling that carries meaning: italics, small caps, alignment, verse. Its typesetter breaks lines with the Knuth–Plass algorithm from TeX, a paragraph at a time, with hyphenation for English, Latin and Greek. Each chapter is laid out once and cached, so pages turn quickly. Fonts are bundled (Junicode, EB Garamond, Cardo, Literata, Atkinson Hyperlegible), with Noto fonts for the Hebrew, Arabic and Sanskrit words that dictionaries cite.

Dictionaries are SQLite packs, built by `scripts/build-dictionaries.sh` from [kaikki.org](https://kaikki.org/)'s extracts of Wiktionary. There are twenty: English, Latin, Ancient Greek, Old English and Old Norse for reading, and fifteen older languages, back to Proto-Indo-European, for following etymologies. Saved words are reviewed with the FSRS algorithm.

Notes and saved words are append-only JSON Lines logs. Underlines and ink are anchored to the words they belong to, not to page positions, so they survive any change of type. A desktop build runs the same app in a window, for development, and can record a session for a video (`scripts/replay.py`). Wordhord also compiles parallel editions of old texts, an original and its translation aligned stanza by stanza; `books/` has five.

[SPEC.md](SPEC.md) has the full design: what's done, what's next, and why.

## Credits and licence

Wordhord is free software under the [GNU GPL, version 3](LICENSE).

Its typefaces are free fonts, most under the SIL Open Font License; see [fonts/README.md](fonts/README.md). The dictionaries come from Wiktionary, under CC BY-SA. The sleep screen is made from a photograph of Knud Baade's *Scene from the Era of Norwegian Sagas* (1850), CC BY-SA 3.0; see [art/README.md](art/README.md).

The screenshots and recordings show [Standard Ebooks](https://standardebooks.org/)' public-domain editions of Dryden's *Aeneid* and Melville's *Moby-Dick*, and Virgil's Latin from [Project Gutenberg](https://www.gutenberg.org/ebooks/227). Much of how Wordhord talks to a Kobo was learned from [KOReader](https://github.com/koreader/koreader).
