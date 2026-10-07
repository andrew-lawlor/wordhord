#!/bin/sh
# Builds the reader for a Kobo and lays out what goes on the device, as
# it sits there: dist/kobo/.adds/wordhord and dist/kobo/.adds/nm/wordhord.
#
#   scripts/package-kobo.sh <book.epub>... [--edition <folder> <chapter> [<translation>]]
#
# EPUBs go into the library as they are: the Kobo lays them out itself.
# An edition (Djot source) is typeset here, with a translation if given.
# The last book named opens next.
#
# Needs the armv7-unknown-linux-musleabihf Rust target, cargo-zigbuild, and
# zig (pip's ziglang) in ~/.cache/wordhord-tools/venv.
set -eu
cd "$(dirname "$0")/.."
PATH="$HOME/.cache/wordhord-tools/venv/bin:$PATH"
OUT=dist/kobo
APP="$OUT/.adds/wordhord"

if [ $# -lt 1 ]; then
	echo "usage: $0 <book.epub>... [--edition <folder> <chapter> [<translation>]]" >&2
	exit 2
fi

rm -rf "${OUT:?}"
mkdir -p "$APP/library" "$OUT/.adds/nm"

cargo zigbuild --quiet --release --target armv7-unknown-linux-musleabihf -p wordhord-device
cargo build --quiet --release -p wordhord-cli
cp target/armv7-unknown-linux-musleabihf/release/wordhord-reader "$APP/"
cp crates/wordhord-device/kobo/wordhord.sh "$APP/"
cp crates/wordhord-device/kobo/nickelmenu "$OUT/.adds/nm/wordhord"
scripts/app-folder.sh "$APP"

last=
while [ $# -gt 0 ]; do
	case "$1" in
	--edition)
		book=$2
		chapter=$3
		shift 3
		last="$(basename "$book")-$chapter"
		if [ $# -gt 0 ] && [ -d "$1" ]; then
			target/release/wordhord typeset "$book" "$chapter" profiles/elipsa-2e.toml \
				"$APP/library/$last" --no-preview --translation "$1"
			shift
		else
			target/release/wordhord typeset "$book" "$chapter" profiles/elipsa-2e.toml \
				"$APP/library/$last" --no-preview
		fi
		;;
	*)
		last="$(basename "$1")"
		cp "$1" "$APP/library/$last"
		shift
		;;
	esac
done
printf '%s' "$last" >"$APP/library/last"

du -sh "$APP"
find "$OUT" -type f | sort
