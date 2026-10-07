#!/bin/sh
# Opens Wordhord in a window on the desktop, with a folder of EPUBs (a
# Calibre library works) as its library.
#
#   scripts/try-desktop.sh <folder>
#
# The app folder, with its index and cache, is ~/.cache/wordhord-dev/app.
# A click is a tap, a drag a swipe, a long press a hold; the arrow keys
# turn pages and Escape closes.
set -eu
cd "$(dirname "$0")/.."
if [ $# -ne 1 ]; then
	echo "usage: $0 <folder of books>" >&2
	exit 2
fi
APP="$HOME/.cache/wordhord-dev/app"
cargo build --quiet --release -p wordhord-device
scripts/app-folder.sh "$APP"
exec target/release/wordhord-reader "$1" "$APP"
