#!/bin/sh
# Opens the reader in a window on the desktop, on chapters of real EPUBs.
#
#   scripts/try-desktop.sh <book.epub> <document> [<book.epub> <document>]...
#
# Each document (a number from `wordhord epub <book.epub>`) becomes a book
# in a library under ~/.cache/wordhord-dev; the last one opens. A click is
# a tap, a drag a swipe, a long press a hold; the arrow keys turn pages
# and Escape closes.
set -eu
cd "$(dirname "$0")/.."
DEV="$HOME/.cache/wordhord-dev"
if [ $# -lt 2 ]; then
	echo "usage: $0 <book.epub> <document> [<book.epub> <document>]..." >&2
	exit 2
fi
cargo build --quiet --release -p wordhord-cli -p wordhord-device
mkdir -p "$DEV/library" "$DEV/fonts"
cp fonts/junicode/*.otf fonts/dejavu/*.ttf "$DEV/fonts/"
name=
while [ $# -ge 2 ]; do
	name="$(basename "$1" .epub | tr -c 'A-Za-z0-9\n' '-')-$2"
	rm -rf "${DEV:?}/library/$name"
	target/release/wordhord epub "$1" "$2" profiles/elipsa-2e.toml "$DEV/library/$name"
	shift 2
done
printf '%s' "$name" >"$DEV/library/last"
exec target/release/wordhord-reader "$DEV/library" "$DEV/fonts"
