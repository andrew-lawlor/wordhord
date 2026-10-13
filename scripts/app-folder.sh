#!/bin/sh
# Makes the reader's app folder: a profile for each Kobo it knows (the
# reader takes the one for its screen), the Elipsa 2E's to begin, with the fonts
# beside it. Keeps any cache already there.
#
#   scripts/app-folder.sh <folder>
set -eu
cd "$(dirname "$0")/.."
APP=$1
mkdir -p "$APP/fonts"
# Every face the type settings offer, flat, each with its licence.
cp fonts/dejavu/DejaVuSerif.ttf fonts/dejavu/DejaVuSansMono.ttf "$APP/fonts/"
cp fonts/dejavu/LICENSE "$APP/fonts/DejaVu-LICENSE.txt"
for face in junicode ebgaramond cardo literata atkinson noto; do
	cp fonts/$face/*.otf fonts/$face/*.ttf "$APP/fonts/" 2>/dev/null || true
	cp fonts/$face/OFL.txt "$APP/fonts/$face-OFL.txt"
done
# The sleep screen: Knud Baade's painting, with its credit.
cp art/sleep-*.png "$APP/"
cp art/README.md "$APP/sleep-credit.txt"
sed -e 's|"\.\./fonts/[a-z]*/|"fonts/|' profiles/elipsa-2e.toml >"$APP/profile.toml"
mkdir -p "$APP/profiles"
for p in profiles/*.toml; do
	sed -e 's|"\.\./fonts/[a-z]*/|"../fonts/|' "$p" >"$APP/profiles/$(basename "$p")"
done
# Dictionary packs built with `wordhord dictionary`, if any.
mkdir -p "$APP/dictionaries"
for pack in "$HOME"/.cache/wordhord-tools/kaikki/*.sqlite; do
	if [ -e "$pack" ]; then cp "$pack" "$APP/dictionaries/"; fi
done
