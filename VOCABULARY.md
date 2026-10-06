# Wordhord vocabulary

Draft 1, 2026-10-06.

Syntax is [Djot](https://djot.net). This file defines what the syntax means in a book. Each element maps to its TEI counterpart, so a Wordhord book can be exported to TEI and back.

Anything not listed here is rejected by the validator.

## Syntax rules

Checked against jotdown 0.7, the Rust Djot parser.

- A container takes one class name: `::: verse`. Nothing else goes on that line.
- Attributes go on their own line, just before the block: `{#ch01-0001 ref=bw start=1}`.
- Quote any value with a dot in it: `ref="vsp.1"`. Unquoted, the whole attribute line silently becomes text. The validator must catch this.
- A heading's ID goes on the line before it, not after it.
- Leading spaces inside a paragraph are dropped by the parser. The compiler reads verse lines from the source text instead (see Verse lines).

## Blocks

| Element | Djot | Meaning | TEI |
|---|---|---|---|
| Heading | `#`, `##`, … | Part, chapter, section | `<head>` in `<div>` |
| Paragraph | plain text | Prose paragraph | `<p>` |
| Verse | `::: verse` | Lines of verse. One source line is one verse line. | `<lg>` with `<l>` |
| Stanza | `::: stanza` | A stanza, inside or instead of verse | `<lg type="stanza">` |
| Speech | `::: speech` | A speech, with `speaker=` | `<sp>` with `<speaker>` |
| Epigraph | `::: epigraph` | Epigraph. A last paragraph with `{.cite}` is its source. | `<epigraph>`, `<bibl>` |
| Letter | `::: letter` | A letter within the text | `<floatingText type="letter">` |
| Quotation | `>` | Block quotation | `<quote>` |
| Centred | `::: center` | Centred text, kept only where it carries meaning | `rend="center"` |
| Scene break | `* * *` | Section break within a chapter | `<milestone unit="section"/>` |
| Figure | `::: figure` | An image, with an optional caption paragraph | `<figure>`, `<head>` |
| Table | Djot table | A table | `<table>` |
| Note | `[^id]` and its definition | Footnote or endnote | `<note>` |
| Commentary | `::: commentary` | A note on a passage, with `on=` its reference: `on="vsp.1"`, `on="vsp.5-6"` | `<note type="commentary" target>` |

## Inlines

| Element | Djot | Meaning | TEI |
|---|---|---|---|
| Emphasis | `_text_` | Emphasis, usually italic | `<emph>` |
| Strong | `*text*` | Strong emphasis | `<hi rend="bold">` |
| Small caps | `[text]{.sc}` | Small capitals | `<hi rend="small-caps">` |
| Foreign | `[text]{lang=non}` | Text in another language. ISO 639-3 codes. | `<foreign xml:lang>` |
| Caesura | ` \| ` in verse | The break between half-lines, set as a gap | `<caesura/>` |
| Gap | `[…]{.gap}` | Text lost or illegible in the source | `<gap/>` |
| Link | `[text](#id)` | Cross-reference within the book | `<ref target>` |
| Line override | `[text]{n=12-13}` | This verse line covers these original lines | `@n` on `<l>` |
| Person | `[text]{.person key=…}` | A person, god or being | `<persName ref>` |
| Place | `[text]{.place key=…}` | A place | `<placeName ref>` |
| Group | `[text]{.group key=…}` | A people, family or band | `<orgName ref>` |
| Named thing | `[text]{.named key=… type=…}` | Any other named thing: a ship, a sword, a hall | `<name type ref>` |
| Source page | `[]{.pb n=24}` | A page break in the source edition. Empty. | `<pb n ed>` |
| Section start | `[]{.ref n="nh.12.42"}` | A canonical section starts here, mid-block. Empty. | `<milestone unit n>` |

## Attributes

| Attribute | On | Meaning |
|---|---|---|
| `#id` | any block | Stable ID. Set at import, never changed, never reused. |
| `lang=` | block, span | Language, ISO 639-3: `eng`, `ang` (Old English), `non` (Old Norse), `grc`, `lat`. |
| `ref=` | verse, stanza, any block | Canonical reference: a prefix for counted verse (`bw`), or a full reference (`"vsp.1"`, `"nh.12.42"`). |
| `start=` | verse | Number of the first line. Later lines are counted. |
| `original=` | verse, stanza | ID of the original book this aligns to (from its `book.toml`). |
| `speaker=` | speech | Who speaks. |
| `on=` | commentary | The reference of the passage it's about. Ranges allowed. |
| `indent=` | verse | Extra indent for the whole block, in steps. |
| `key=` | name spans | Entry in the book's name register. |
| `type=` | `.named` | What kind of thing: `ship`, `weapon`, `hall`, … |
| `ed=` | `.pb` | Which source edition, when the book records more than one. Default: the book's `[edition]`. |

## Verse lines

Inside `::: verse` or `::: stanza`, each source line is one line of verse. Djot reads the lines as one paragraph and drops leading spaces, so the compiler reads them from the source text by offset. Two spaces are one indent step.

```
{#ch01-0002}
::: verse
Tell me, O Muse, of that ingenious hero
  who travelled far and wide after he had sacked
  the famous town of Troy.
:::
```

` | ` marks a caesura. It's set as a gap, not printed.

## References

`book.toml` declares the scheme:

```toml
[refs]
prefix = "bw"
unit = "line"        # or "stanza", or "stanza.line"
```

A book of several works (an anthology, the Poetic Edda) lists them instead of one prefix:

```toml
[refs]
unit = "stanza.line"
works = { vsp = "Völuspá", hav = "Hávamál", grm = "Grímnismál" }
```

Counted verse can sit under a book or canto of the work. Homer is cited by book and line, Dante by canto and line:

```
{#ch01-0001 ref="od.1" start=1}
::: verse
Tell me about a complicated man.
:::
```

numbers its lines `od.1.1`, `od.1.2`, and so on.

For prose, list the levels:

```toml
[refs]
prefix = "nh"
levels = ["book", "section"]
```

Forms:
- `bw.1`: line 1.
- `bw.4-5`: lines 4 to 5.
- `od.1.1`: *Odyssey*, book 1, line 1.
- `inf.34.139`: *Inferno*, canto 34, line 139.
- `vsp.1`: stanza 1 of *Völuspá*.
- `vsp.1.3`: stanza 1, line 3.
- `nh.12.42`: Pliny, *Natural History*, book 12, section 42.

Quote every reference with a dot in it.

Lines in a `ref` block are counted from `start=`. An override (`{n=…}`) changes the count for its line only. The next line continues after the override's last number.

### Prose

A prose reference covers text until the next one starts.

- When a section starts with a block, put `ref=` on the block: `{#ch12-0007 ref="nh.12.41"}`.
- When it starts mid-block, put an empty span where it starts: `[]{.ref n="nh.12.42"}`.

Both forms can mix in one book. The compiler turns them into one list of ranges.

## Alignment

A translation names its original with `original=` and shares its references. The compiler matches them.

- Line schemes match line to line.
- Stanza schemes match stanza to stanza, however many lines each has.
- The validator warns when a translation skips a reference, repeats one, or runs past the original's end.

## IDs

Every block gets `#id` at import. The form is `<chapter>-<n>`: `ch01-0042`. New blocks added later take the next unused number. Deleted IDs are never reused.

Annotations anchor to IDs plus a quote. Canonical references are separate: an ID says where text is in this book; a reference says where it is in the work.

## Names

Names are tagged by hand or by a later tool, never guessed at import. Tagging is optional, book by book.

```
From [Heimdall]{.person key=heimdallr}'s sons, | both high and low;
```

`key=` points to the book's register, `names.toml`:

```toml
[heimdallr]
name = "Heimdallr"
kind = "person"
also = ["Heimdall", "Heimdal"]
note = "Watchman of the gods."
wikidata = "Q…"          # look up before use
```

- A key without an entry is an error. An entry without any use is a warning.
- The same key across books (an original and its translations) means the same being. Registers can be shared by a corpus later.
- `wikidata=` is optional. It links names to the outside world without depending on it.

## Source pages

A book can record the pages of the printed edition it came from, so a passage can be cited the way a library copy would be.

`book.toml` names the edition:

```toml
[edition]
key = "gummere-1910"
title = "The Oldest English Epic"
publisher = "Macmillan"
year = 1910
```

Then empty spans mark where each page starts:

```
…and what honor the athelings won! []{.pb n=2}Oft Scyld the Scefing…
```

- A page break can fall inside a word: `prow[]{.pb n=25}ess`. The compiler joins the halves into one word.
- With more than one edition, add `ed=`: `[]{.pb n=25 ed=klaeber-1922}`.
- Source pages are for citation only. They never decide where the device's pages break.
