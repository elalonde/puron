#!/bin/sh
# Builds the Debian package target/deb/puron_<version>_<arch>.deb with
# dpkg-deb from a staged make install (PREFIX=/usr). Depends is
# libsensors5 plus what dpkg-shlibdeps finds for the two binaries. The
# binaries are built with the cargo home and the build directory remapped
# to /cargo and /build, so they hold neither real path, and then
# stripped. /usr/share/doc/puron gets the copyright file, the changelog,
# the GPL-3.0 text, the source offer and the third-party notices.
# Usage: tools/make-deb.sh   (or make deb)
cd "$(dirname "$0")/.." || exit 2
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$PWD/$target ;; esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
arch=$(dpkg --print-architecture) || exit 2
[ -n "$version" ] || { echo "make-deb: no version in Cargo.toml"; exit 2; }
work=$target/deb/work
stage=$work/root
rm -rf "$work" && mkdir -p "$stage" || exit 2
cargo_home=${CARGO_HOME:-$HOME/.cargo}
RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$PWD=/build --remap-path-prefix=$target=/build/target"
export RUSTFLAGS
make -s install DESTDIR="$stage" PREFIX=/usr || exit 1
strip --strip-unneeded "$stage/usr/bin/puron-applet" "$stage/usr/bin/puron-settings" || exit 1
install -d "$stage/usr/share/doc/puron" || exit 2
install -m 644 debian/copyright "$stage/usr/share/doc/puron/copyright" || exit 2
gzip -9n < debian/changelog > "$stage/usr/share/doc/puron/changelog.gz" || exit 2
install -m 644 data/licenses/GPL-3.0.txt "$stage/usr/share/doc/puron/GPL-3.0.txt" || exit 2
sed "s|@VERSION@|$version|g" debian/source-offer.in > "$stage/usr/share/doc/puron/SOURCE-OFFER" || exit 2
sh tools/vendor-tree.sh "$work/tree" "$work/scratch" || exit 1
python3 -I tools/gen-notices.py "$work/tree/vendor" "$work/scratch/metadata.json" \
    "$stage/usr/share/doc/puron/THIRD-PARTY-NOTICES.txt" || exit 1
rm -rf "$work/tree" "$work/scratch"
chmod 644 "$stage"/usr/share/doc/puron/*
# dpkg-shlibdeps must run in a source tree with a debian/control file; a
# minimal one in the work directory is enough.
mkdir -p "$work/src/debian" || exit 2
printf 'Source: puron\n\nPackage: puron\nArchitecture: any\n' > "$work/src/debian/control"
shl=$(cd "$work/src" && dpkg-shlibdeps -O -e"$stage/usr/bin/puron-applet" -e"$stage/usr/bin/puron-settings" 2> ../shlibdeps.err) \
    || { cat "$work/shlibdeps.err"; exit 1; }
deps=$(printf '%s\n' "$shl" | sed -n 's/^shlibs:Depends=//p')
case ", $deps," in
*", libsensors5"[\ ,]*) all=$deps ;;
*) all="libsensors5${deps:+, $deps}" ;;
esac
size=$(du -sk "$stage/usr" | cut -f1)
mkdir -p "$stage/DEBIAN" || exit 2
sed -e "s|@VERSION@|$version|" -e "s|@ARCH@|$arch|" -e "s|@SIZE@|$size|" -e "s|@DEPENDS@|$all|" \
    debian/control.in > "$stage/DEBIAN/control" || exit 2
(cd "$stage" && find usr -type f | LC_ALL=C sort | xargs md5sum) > "$stage/DEBIAN/md5sums" || exit 2
find "$stage" -type d -exec chmod 755 {} +
out=$target/deb/puron_${version}_${arch}.deb
dpkg-deb --root-owner-group -Zxz --build "$stage" "$out" > /dev/null || exit 1
echo "make-deb: $out"
