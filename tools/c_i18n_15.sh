#!/bin/sh
# Checks that the normal dependency trees of both GUI crates hold
# i18n-embed, i18n-embed-fl and rust-embed at the pinned versions and at
# no other version: each crate must give exactly the three lines below
# (diff exits 0).
# Usage: tools/c_i18n_15.sh
cd "$(dirname "$0")/.." || exit 2
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
printf '%s\n' 'i18n-embed v0.16.0' 'i18n-embed-fl v0.10.1' 'rust-embed v8.13.0' > "$tmp/want"
rc=0
for p in puron-applet puron-settings; do
    cargo tree -p "$p" -e normal --prefix none --format '{p}' > "$tmp/tree" || { echo "C-i18n-15: cargo tree $p failed"; exit 1; }
    grep -oE '^(i18n-embed|i18n-embed-fl|rust-embed) v[0-9.]+' "$tmp/tree" | LC_ALL=C sort -u > "$tmp/got"
    if diff "$tmp/want" "$tmp/got"; then echo "C-i18n-15: $p ok"; else echo "C-i18n-15: $p FAIL"; rc=1; fi
done
exit $rc
