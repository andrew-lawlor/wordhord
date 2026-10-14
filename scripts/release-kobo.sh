#!/bin/sh
# Builds Wordhord for a Kobo release from a commit, in a clean clone, so
# nothing on this machine but the commit goes into it: the package without
# dictionaries (they're a release of their own), zipped to unzip onto the
# Kobo's root, with its checksum. Into dist/release/.
#
#   scripts/release-kobo.sh [commit or tag]   (HEAD by default)
#
# Needs what scripts/package-kobo.sh needs.
set -eu
cd "$(dirname "$0")/.."
REPO=$(pwd)
REF=${1:-HEAD}
COMMIT=$(git rev-parse --verify "$REF^{commit}")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

git clone --quiet "$REPO" "$WORK/src"
cd "$WORK/src"
git checkout --quiet "$COMMIT"
git submodule --quiet update --init --recursive
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

# Built with no dictionaries to hand, so none are copied in.
mkdir -p "$WORK/home"
HOME_REAL=$HOME
HOME="$WORK/home" CARGO_HOME="${CARGO_HOME:-$HOME_REAL/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME_REAL/.rustup}" \
	PATH="$HOME_REAL/.cache/wordhord-tools/venv/bin:$PATH" scripts/package-kobo.sh >/dev/null

NAME="wordhord-$VERSION-kobo.zip"
mkdir -p "$REPO/dist/release"
rm -f "$REPO/dist/release/$NAME"
(cd dist/kobo && zip -q -r -9 "$REPO/dist/release/$NAME" .adds)
cd "$REPO/dist/release"
sha256sum "$NAME" >SHA256SUMS
echo "$NAME from $(git -C "$REPO" log -1 --format='%h %s' "$COMMIT")"
unzip -l "$NAME" | tail -1
cat SHA256SUMS
