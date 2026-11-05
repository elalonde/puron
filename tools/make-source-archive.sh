#!/bin/sh
# Builds target/source/puron-<version>-source.tar.xz, the complete
# corresponding source of a release: the tree at HEAD, the locked
# dependencies vendored with the cargo config for an offline build, and
# the third-party notices.
# Usage: tools/make-source-archive.sh   (or make source-archive)
cd "$(dirname "$0")/.." || exit 2
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$PWD/$target ;; esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "make-source-archive: no version in Cargo.toml"; exit 2; }
name=puron-$version
work=$target/source
rm -rf "$work" && mkdir -p "$work" || exit 2
git archive --prefix="$name/" HEAD | tar -xf - -C "$work" || exit 2
sh tools/vendor-tree.sh "$work/$name" "$work/scratch" || exit 1
python3 -I tools/gen-notices.py "$work/$name/vendor" "$work/scratch/metadata.json" \
    "$work/$name/THIRD-PARTY-NOTICES.txt" || exit 1
out=$work/$name-source.tar.xz
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$(git log -1 --format=%ct)" \
    -C "$work" -cf - "$name" | xz -T0 -6 > "$out" || exit 1
rm -rf "${work:?}/$name" "$work/scratch"
echo "make-source-archive: $out"
