# Hyphenation patterns

English patterns come embedded in the `hyphenation` crate. These two are
copied from it (version 0.8.4), so only the languages Wordhord sets are
carried, not all of them:

- `grc.standard.bincode`: Ancient Greek, from `hyph-grc` by Dimitrios
  Filippou.
- `la-x-classic.standard.bincode`: classical Latin, from
  `hyph-la-x-classic` by Claudio Beccari.

Both are TeX's patterns, distributed in
[hyph-utf8](https://github.com/hyphenation/tex-hyphen) under their own
licences (the LaTeX Project Public License), compiled by the `hyphenation`
crate.
