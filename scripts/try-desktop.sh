#!/bin/sh
# Opens the reader in a window on the desktop, on whole EPUBs.
#
#   scripts/try-desktop.sh <book.epub>...
#
# The books are linked into a library under ~/.cache/wordhord-dev; the
# last one opens. Chapters are laid out as they're opened and cached in
# ~/.cache/wordhord-dev/app/cache. A click is a tap, a drag a swipe, a long
# press a hold; the arrow keys turn pages and Escape closes.
set -eu
cd "$(dirname "$0")/.."
DEV="$HOME/.cache/wordhord-dev"
if [ $# -lt 1 ]; then
	echo "usage: $0 <book.epub>..." >&2
	exit 2
fi
cargo build --quiet --release -p wordhord-device
scripts/app-folder.sh "$DEV/app"
mkdir -p "$DEV/library"
name=
for book in "$@"; do
	name="$(basename "$book")"
	ln -sf "$(realpath "$book")" "$DEV/library/$name"
done
printf '%s' "$name" >"$DEV/library/last"
exec target/release/wordhord-reader "$DEV/library" "$DEV/app"
