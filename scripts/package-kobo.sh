#!/bin/sh
# Builds the reader for a Kobo and lays out what goes on the device, as
# it sits there: dist/kobo/.adds/wordhord and dist/kobo/.adds/nm/wordhord.
#
#   scripts/package-kobo.sh <book folder> <chapter> [<translation folder>]
#
# With a translation, each stanza's number opens the same stanza in it. The
# book becomes the one the reader opens next.
#
# Needs the armv7-unknown-linux-musleabihf Rust target, cargo-zigbuild, and
# zig (pip's ziglang) in ~/.cache/wordhord-tools/venv.
set -eu
cd "$(dirname "$0")/.."
PATH="$HOME/.cache/wordhord-tools/venv/bin:$PATH"
OUT=dist/kobo
APP="$OUT/.adds/wordhord"

if [ $# -lt 2 ] || [ $# -gt 3 ]; then
	echo "usage: $0 <book folder> <chapter> [<translation folder>]" >&2
	exit 2
fi
book=$1
chapter=$2
name="$(basename "$book")-$chapter"

rm -rf "${OUT:?}"
mkdir -p "$APP/library" "$APP/fonts" "$OUT/.adds/nm"

cargo zigbuild --quiet --release --target armv7-unknown-linux-musleabihf -p wordhord-device
cargo build --quiet --release -p wordhord-cli
cp target/armv7-unknown-linux-musleabihf/release/wordhord-reader "$APP/"
cp crates/wordhord-device/kobo/wordhord.sh "$APP/"
cp crates/wordhord-device/kobo/nickelmenu "$OUT/.adds/nm/wordhord"

if [ $# -eq 3 ]; then
	target/release/wordhord typeset "$book" "$chapter" profiles/elipsa-2e.toml \
		"$APP/library/$name" --no-preview --translation "$3"
else
	target/release/wordhord typeset "$book" "$chapter" profiles/elipsa-2e.toml \
		"$APP/library/$name" --no-preview
fi
printf '%s' "$name" >"$APP/library/last"

# The profile's fonts, with their licences.
cp fonts/junicode/Junicode-*.otf fonts/dejavu/DejaVuSerif.ttf "$APP/fonts/"
cp fonts/junicode/OFL.txt "$APP/fonts/Junicode-OFL.txt"
cp fonts/dejavu/LICENSE "$APP/fonts/DejaVu-LICENSE.txt"

du -sh "$APP"
find "$OUT" -type f | sort
