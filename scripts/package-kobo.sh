#!/bin/sh
# Builds the reader for a Kobo and lays out what goes on the device, as
# it sits there: dist/kobo/.adds/wordhord and dist/kobo/.adds/nm/wordhord.
#
#   scripts/package-kobo.sh
#
# The reader finds every book on the Kobo itself, wherever Calibre put it,
# so no books are packaged.
#
# Needs the armv7-unknown-linux-musleabihf Rust target, cargo-zigbuild, and
# zig (pip's ziglang) in ~/.cache/wordhord-tools/venv.
set -eu
cd "$(dirname "$0")/.."
PATH="$HOME/.cache/wordhord-tools/venv/bin:$PATH"
OUT=dist/kobo
APP="$OUT/.adds/wordhord"

rm -rf "${OUT:?}"
mkdir -p "$APP" "$OUT/.adds/nm"

cargo zigbuild --quiet --release --target armv7-unknown-linux-musleabihf -p wordhord-device
cp target/armv7-unknown-linux-musleabihf/release/wordhord-reader "$APP/"
cp crates/wordhord-device/kobo/wordhord.sh "$APP/"
cp crates/wordhord-device/kobo/nickelmenu "$OUT/.adds/nm/wordhord"
scripts/app-folder.sh "$APP"

du -sh "$APP"
find "$OUT" -type f | sort
