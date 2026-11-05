#!/bin/sh
# Extracts a source archive into a temporary folder and builds it with
# cargo build --offline --locked --release, under a cargo home of its
# own, so that nothing but the archive can feed the build. The exit code
# is the verdict.
# Usage: tools/check-source-archive.sh [archive]
cd "$(dirname "$0")/.." || exit 2
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$PWD/$target ;; esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
archive=${1:-$target/source/puron-$version-source.tar.xz}
[ -f "$archive" ] || { echo "check-source-archive: no $archive (run make source-archive)"; exit 1; }
mkdir -p "$target" || exit 2
tmp=$(mktemp -d "$target/source-check.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT
tar -xf "$archive" -C "$tmp" || { echo "check-source-archive: cannot extract"; exit 1; }
top=$(find "$tmp" -mindepth 1 -maxdepth 1 -type d)
[ "$(printf '%s\n' "$top" | wc -l)" = 1 ] && [ -n "$top" ] || { echo "check-source-archive: not one top folder"; exit 1; }
for p in Cargo.lock .cargo/config.toml vendor THIRD-PARTY-NOTICES.txt; do
    [ -e "$top/$p" ] || { echo "check-source-archive: no $p"; exit 1; }
done
(cd "$top" && unset RUSTFLAGS && CARGO_HOME=$tmp/cargo-home CARGO_TARGET_DIR=$tmp/target \
    cargo build --offline --locked --release) || { echo "check-source-archive: build failed"; exit 1; }
echo "check-source-archive: ok ($archive)"
