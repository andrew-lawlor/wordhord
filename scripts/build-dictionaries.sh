#!/bin/sh
# Builds Wordhord's dictionary packs from kaikki.org's extracts of
# Wiktionary: downloads each language's (compressed), and builds its pack.
#
#   scripts/build-dictionaries.sh [code ...]
#
# Without codes, every language below. Downloads and packs go in
# ~/.cache/wordhord-tools/kaikki/; a download already there is used again.
set -eu
cd "$(dirname "$0")/.."
OUT="$HOME/.cache/wordhord-tools/kaikki"
mkdir -p "$OUT"
cargo build --quiet --release --bin wordhord

# code, then the language as kaikki names it.
LANGUAGES='
eng English
ang Old English
non Old Norse
lat Latin
grc Ancient Greek
enm Middle English
fro Old French
frm Middle French
goh Old High German
osx Old Saxon
ofs Old Frisian
dum Middle Dutch
got Gothic
sga Old Irish
chu Old Church Slavonic
san Sanskrit
gem-pro Proto-Germanic
gmw-pro Proto-West Germanic
itc-pro Proto-Italic
ine-pro Proto-Indo-European
'

while read -r code name; do
	[ -n "$code" ] || continue
	if [ $# -gt 0 ]; then
		case " $* " in *" $code "*) ;; *) continue ;; esac
	fi
	# kaikki's file names drop spaces and hyphens: ProtoGermanic.
	file=$(echo "$name" | tr -d ' -')
	url="https://kaikki.org/dictionary/$(echo "$name" | sed 's/ /%20/g')/kaikki.org-dictionary-$file.jsonl.gz"
	if [ ! -e "$OUT/$code.jsonl" ]; then
		echo "$code: downloading $name"
		curl -sfL -o "$OUT/$code.jsonl.gz.part" "$url"
		mv "$OUT/$code.jsonl.gz.part" "$OUT/$code.jsonl.gz"
		gunzip -f "$OUT/$code.jsonl.gz"
	fi
	target/release/wordhord dictionary "$OUT/$code.jsonl" "$code" "$OUT/$code.sqlite"
done <<END
$LANGUAGES
END
