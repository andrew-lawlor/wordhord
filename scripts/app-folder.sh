#!/bin/sh
# Makes the reader's app folder: the Elipsa 2E profile, with its fonts
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
for face in junicode ebgaramond cardo literata atkinson; do
	cp fonts/$face/*.otf fonts/$face/*.ttf "$APP/fonts/" 2>/dev/null || true
	cp fonts/$face/OFL.txt "$APP/fonts/$face-OFL.txt"
done
sed -e 's|"\.\./fonts/[a-z]*/|"fonts/|' profiles/elipsa-2e.toml >"$APP/profile.toml"
# Dictionary packs built with `wordhord dictionary`, if any.
mkdir -p "$APP/dictionaries"
for pack in "$HOME"/.cache/wordhord-tools/kaikki/*.sqlite; do
	[ -e "$pack" ] && cp "$pack" "$APP/dictionaries/"
done
