#!/bin/sh
# Makes the reader's app folder: the Elipsa 2E profile, with its fonts
# beside it. Keeps any cache already there.
#
#   scripts/app-folder.sh <folder>
set -eu
cd "$(dirname "$0")/.."
APP=$1
mkdir -p "$APP/fonts"
cp fonts/junicode/Junicode-*.otf fonts/dejavu/DejaVuSerif.ttf fonts/dejavu/DejaVuSansMono.ttf "$APP/fonts/"
cp fonts/junicode/OFL.txt "$APP/fonts/Junicode-OFL.txt"
cp fonts/dejavu/LICENSE "$APP/fonts/DejaVu-LICENSE.txt"
sed -e 's|"\.\./fonts/junicode/|"fonts/|' -e 's|"\.\./fonts/dejavu/|"fonts/|' profiles/elipsa-2e.toml >"$APP/profile.toml"
