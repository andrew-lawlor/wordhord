# Fonts

Bundled, so a book typesets the same on any machine. Each compiled book pins its fonts by hash.

| Font | Files | Licence |
|---|---|---|
| [Junicode](https://github.com/psb1558/Junicode-font) 2.226, by Peter S. Baker | `junicode/` | SIL Open Font License 1.1 (`junicode/OFL.txt`) |
| [EB Garamond](https://github.com/octaviopardo/EBGaramond12), by Georg Duffner and Octavio Pardo | `ebgaramond/` | SIL Open Font License 1.1 (`ebgaramond/OFL.txt`) |
| Cardo, by David J. Perry | `cardo/` | SIL Open Font License 1.1 (`cardo/OFL.txt`) |
| [Literata](https://github.com/googlefonts/literata), by TypeTogether | `literata/` | SIL Open Font License 1.1 (`literata/OFL.txt`) |
| Atkinson Hyperlegible, by the Braille Institute | `atkinson/` | SIL Open Font License 1.1 (`atkinson/OFL.txt`) |
| DejaVu Serif, for the few characters a face lacks (⋮) | `dejavu/` | Bitstream Vera licence, public domain changes (`dejavu/LICENSE`) |
| DejaVu Sans Mono, for code | `dejavu/` | The same Bitstream Vera licence (`dejavu/LICENSE`) |

Junicode is made for medievalists: it has þ, ð, ǫ, æ, œ and the rest of what Old Norse, Old English and Middle English need. Every character in `books/` is in Junicode except U+22EE (⋮). It sets the library and the dictionary, whatever face a book is read in.

The other four are choices in the reading settings. All four came from [google/fonts](https://github.com/google/fonts) (`ofl/`). Cardo and Atkinson Hyperlegible are its static files, unchanged; Cardo has no bold italic, so its bold stands in. EB Garamond and Literata come only as variable fonts there, so the files here are static instances cut with fontTools, at weights 400 and 700, and for Literata at the 12 pt optical size:

```python
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
font = TTFont("Literata[opsz,wght].ttf")
instancer.instantiateVariableFont(font, {"wght": 700, "opsz": 12}, updateFontNames=True).save("Literata-Bold.ttf")
```

Which old languages each face can set is worked out on the device, from the face itself.
