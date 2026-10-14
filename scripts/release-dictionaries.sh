#!/bin/sh
# Readies the dictionary packs for a release: each zipped with a note of
# where it came from and its licence, and their checksums. Into
# dist/dictionaries/. Build the packs first: scripts/build-dictionaries.sh.
#
#   scripts/release-dictionaries.sh
set -eu
cd "$(dirname "$0")/.."
PACKS="$HOME/.cache/wordhord-tools/kaikki"
OUT=dist/dictionaries
rm -rf "$OUT"
mkdir -p "$OUT"
NOTE="$OUT/DICTIONARY-LICENCE.txt"
cat >"$NOTE" <<END
This is a dictionary pack for Wordhord (https://github.com/andrew-lawlor/wordhord).

It is built from Wiktionary (https://www.wiktionary.org/), by way of the
structured extracts at kaikki.org (https://kaikki.org/), made with
wiktextract by Tatu Ylonen. Wiktionary's text is available under the
Creative Commons Attribution-ShareAlike licence, version 4.0
(https://creativecommons.org/licenses/by-sa/4.0/), and the GNU Free
Documentation Licence. So is this pack.

Put the .sqlite file in .adds/wordhord/dictionaries/ on your Kobo.
END
for pack in "$PACKS"/*.sqlite; do
	code=$(basename "$pack" .sqlite)
	cp "$pack" "$OUT/$code.sqlite"
	(cd "$OUT" && zip -q -9 "$code.zip" "$code.sqlite" DICTIONARY-LICENCE.txt && rm "$code.sqlite")
done
rm "$NOTE"
(cd "$OUT" && sha256sum *.zip >SHA256SUMS)
ls -l "$OUT"
