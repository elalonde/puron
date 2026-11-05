#!/bin/sh
# Vendors the crates of Cargo.lock at HEAD into <dest>/vendor and writes
# <dest>/.cargo/config.toml, so that a tree at <dest> builds offline.
#
# cargo vendor refuses Cargo.lock as it stands: cosmic-panel-config asks
# for libcosmic without a revision, the workspace at a pinned one, so the
# same commit is locked under two source URLs. The vendoring therefore
# runs on a scratch copy with one URL, and the config maps both URLs to
# the one vendor directory. Cargo.lock in <dest> is not changed.
#
# Usage: tools/vendor-tree.sh <dest> <scratch>
# <scratch> is created new; it keeps metadata.json for tools/gen-notices.py.
cd "$(dirname "$0")/.." || exit 2
[ $# = 2 ] || { echo "usage: vendor-tree.sh <dest> <scratch>"; exit 2; }
dest=$1; scratch=$2
mkdir -p "$dest/.cargo" || exit 2
dest=$(cd "$dest" && pwd) || exit 2
rm -rf "$scratch" && mkdir -p "$scratch" || exit 2
git archive HEAD Cargo.toml Cargo.lock crates | tar -xf - -C "$scratch" || exit 2
rev=$(sed -n 's/^libcosmic = .*rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)
[ -n "$rev" ] || { echo "vendor-tree: no libcosmic revision in Cargo.toml"; exit 2; }
url=https://github.com/pop-os/libcosmic
sed -i "s/, rev = \"$rev\"//" "$scratch/Cargo.toml" || exit 2
# One URL in the lock, then one entry for each crate that had two.
sed "s|libcosmic?rev=$rev#|libcosmic#|" Cargo.lock \
    | awk 'BEGIN { RS = ""; ORS = "\n\n" } !seen[$1 " " $2 " " $3 " " $4 " " $5 " " $6 " " $7 " " $8 " " $9]++' \
    > "$scratch/Cargo.lock" || exit 2
(cd "$scratch" && cargo vendor --versioned-dirs "$dest/vendor") > "$scratch/config" 2> "$scratch/vendor.log" \
    || { tail -n 20 "$scratch/vendor.log"; exit 1; }
(cd "$scratch" && cargo metadata --offline --format-version 1) > "$scratch/metadata.json" 2> "$scratch/metadata.log" \
    || { tail -n 20 "$scratch/metadata.log"; exit 1; }
{
    sed -e '/^\[source/,$!d' -e 's|^directory = .*|directory = "vendor"|' "$scratch/config"
    printf '\n[source."git+%s?rev=%s"]\ngit = "%s"\nrev = "%s"\nreplace-with = "vendored-sources"\n' \
        "$url" "$rev" "$url" "$rev"
} > "$dest/.cargo/config.toml" || exit 2
