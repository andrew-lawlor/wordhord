#!/bin/sh
# Builds the reader for a Kobo and lays out what goes on the device, as
# it sits there: dist/kobo/.adds/wordhord and dist/kobo/.adds/nm/wordhord.
#
#   scripts/package-kobo.sh <book folder> <chapter> [<book folder> <chapter>]...
#
# Needs the armv7-unknown-linux-musleabihf Rust target, cargo-zigbuild, and
# zig (pip's ziglang) in ~/.cache/wordhord-tools/venv.
set -eu
cd "$(dirname "$0")/.."
PATH="$HOME/.cache/wordhord-tools/venv/bin:$PATH"
OUT=dist/kobo
APP="$OUT/.adds/wordhord"

if [ $# -lt 2 ]; then
	echo "usage: $0 <book folder> <chapter> [<book folder> <chapter>]..." >&2
	exit 2
fi

rm -rf "${OUT:?}"
mkdir -p "$APP/library" "$APP/fonts" "$OUT/.adds/nm"

cargo zigbuild --quiet --release --target armv7-unknown-linux-musleabihf -p wordhord-device
cargo build --quiet --release -p wordhord-cli
cp target/armv7-unknown-linux-musleabihf/release/wordhord-reader "$APP/"
cp crates/wordhord-device/kobo/wordhord.sh "$APP/"
cp crates/wordhord-device/kobo/nickelmenu "$OUT/.adds/nm/wordhord"

while [ $# -ge 2 ]; do
	book=$1
	chapter=$2
	shift 2
	target/release/wordhord typeset "$book" "$chapter" profiles/elipsa-2e.toml \
		"$APP/library/$(basename "$book")-$chapter" --no-preview
done

# The profile's fonts, with their licences.
cp fonts/junicode/Junicode-*.otf fonts/dejavu/DejaVuSerif.ttf "$APP/fonts/"
cp fonts/junicode/OFL.txt "$APP/fonts/Junicode-OFL.txt"
cp fonts/dejavu/LICENSE "$APP/fonts/DejaVu-LICENSE.txt"

du -sh "$APP"
find "$OUT" -type f | sort
