#!/bin/sh
# Reads back the package that make deb built and checks it. It checks:
# - the control fields (package, version, architecture, maintainer, and
#   Depends with libsensors5 and libc6), every expected file and nothing
#   more, the binaries' modes, and the desktop entry's Exec line;
# - the license documents: the GPL-3.0 text, the copyright paragraphs,
#   the source offer, and the third-party notices against Cargo.lock,
#   with the matching statements in README.md;
# - that no file holds this machine's home path and no binary holds any
#   home path. With PURON_TERMS=<file>, the binaries must also hold none
#   of the terms listed there, one per line as "<c|i><TAB><extended
#   regex>" (i: ignore case; lines starting with # are comments).
#   PURON_TERMS_SKIP=<file> lists whole strings that are known to be
#   harmless data and are not searched.
# Usage: tools/check-deb.sh [package file]   (or make check-deb)
cd "$(dirname "$0")/.." || exit 2
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$PWD/$target ;; esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
arch=$(dpkg --print-architecture) || exit 2
deb=${1:-$target/deb/puron_${version}_${arch}.deb}
[ -f "$deb" ] || { echo "check-deb: no $deb (run make deb)"; exit 1; }
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
bad=0
fail() { echo "check-deb: FAIL $*"; bad=1; }
dpkg-deb --info "$deb" > "$tmp/info" || fail "dpkg-deb --info"
dpkg-deb --contents "$deb" > "$tmp/contents" || fail "dpkg-deb --contents"
field() { dpkg-deb --field "$deb" "$1"; }
[ "$(field Package)" = puron ] || fail "Package: $(field Package)"
[ "$(field Version)" = "$version" ] || fail "Version: $(field Version)"
[ "$(field Architecture)" = "$arch" ] || fail "Architecture: $(field Architecture)"
[ "$(field Maintainer)" = "elalonde <eric@lalonde.com>" ] || fail "Maintainer: $(field Maintainer)"
field Depends | tr ',' '\n' | sed 's/^ *//' | grep -qE '^libsensors5( |$)' || fail "Depends has no libsensors5: $(field Depends)"
field Depends | tr ',' '\n' | sed 's/^ *//' | grep -qE '^libc6( |$)' || fail "Depends has no libc6 (dpkg-shlibdeps): $(field Depends)"
grep -q '^ Depends: ' "$tmp/info" || fail "no Depends line in --info"
for p in usr/bin/puron-applet usr/bin/puron-settings \
    usr/share/applications/io.github.elalonde.Puron.desktop \
    usr/share/doc/puron/changelog.gz usr/share/doc/puron/copyright usr/share/doc/puron/GPL-3.0.txt \
    usr/share/doc/puron/SOURCE-OFFER usr/share/doc/puron/THIRD-PARTY-NOTICES.txt; do
    grep -qE " \./$p\$" "$tmp/contents" || fail "missing /$p"
done
for f in data/icons/hicolor/scalable/status/*.svg; do
    p=usr/share/icons/hicolor/scalable/status/${f##*/}
    grep -qE " \./$p\$" "$tmp/contents" || fail "missing /$p"
done
grep -E '^-' "$tmp/contents" | grep -E ' \./usr/bin/' | grep -qvE '^-rwxr-xr-x root/root ' && fail "a binary is not -rwxr-xr-x root/root"
n=$(grep -cE '^-' "$tmp/contents")
want=$(( 8 + $(ls data/icons/hicolor/scalable/status/*.svg | wc -l) ))
[ "$n" = "$want" ] || fail "$n files, want $want"
dpkg-deb -x "$deb" "$tmp/x" || fail "dpkg-deb -x"
grep -qx 'Exec=/usr/bin/puron-applet' "$tmp/x/usr/share/applications/io.github.elalonde.Puron.desktop" \
    || fail "desktop Exec is not /usr/bin/puron-applet"
grep -q '^License: MIT$' "$tmp/x/usr/share/doc/puron/copyright" || fail "copyright has no MIT license"
doc=$tmp/x/usr/share/doc/puron
# The GPL-3.0 statement.
cmp -s "$doc/GPL-3.0.txt" data/licenses/GPL-3.0.txt || fail "GPL-3.0.txt differs from data/licenses/GPL-3.0.txt"
grep -q 'GNU GENERAL PUBLIC LICENSE' data/licenses/GPL-3.0.txt || fail "data/licenses/GPL-3.0.txt is not the GPL"
grep -q '^Files: usr/bin/puron-applet$' "$doc/copyright" || fail "copyright has no paragraph for puron-applet"
grep -q '^Files: usr/bin/puron-settings$' "$doc/copyright" || fail "copyright has no paragraph for puron-settings"
grep -qE '^License: GPL-3\.0-only( |$)' "$doc/copyright" || fail "copyright names no GPL-3.0-only"
for l in GPL-3.0-only MPL-2.0 LGPL-2.1 NVIDIA-NVML-notice; do
    grep -qx "License: $l" "$doc/copyright" || fail "copyright has no $l paragraph"
done
tr '\n' ' ' < README.md | grep -q 'GPL-3.0 combined work' || fail "README has no GPL-3.0 statement"
# The source offer.
asset=puron-$version-source.tar.xz
for f in "$doc/SOURCE-OFFER" README.md; do
    grep -q "$asset" "$f" || fail "$f names no $asset"
    grep -q "v$version" "$f" || fail "$f names no tag v$version"
done
grep -q "releases/tag/v$version" "$doc/SOURCE-OFFER" || fail "SOURCE-OFFER has no release link"
# The third-party notices: a block for every crate of Cargo.lock, each
# with a license, a source and a reference to a license text that the
# file holds; and the NVIDIA notice, unchanged.
notices=$doc/THIRD-PARTY-NOTICES.txt
if [ -f "$notices" ]; then
    awk '/^name = /{ n = $3; next } n != "" && /^version = /{ print n " " $3; n = "" }' Cargo.lock | tr -d '"' | LC_ALL=C sort -u > "$tmp/lock-crates"
    sed -n '/^=== crates begin$/,/^=== crates end$/p' "$notices" | sed '1d;$d' > "$tmp/blocks"
    sed -n 's/^Crate: //p' "$tmp/blocks" | LC_ALL=C sort -u > "$tmp/notice-crates"
    LC_ALL=C comm -23 "$tmp/lock-crates" "$tmp/notice-crates" > "$tmp/missing"
    [ -s "$tmp/lock-crates" ] || fail "no crates read from Cargo.lock"
    [ ! -s "$tmp/missing" ] || fail "crates missing from the notices: $(tr '\n' ',' < "$tmp/missing")"
    awk 'BEGIN { RS = "" } !/^Crate: [^\n]+\nLicense: [^\n]+\nSource: [^\n]+\nText: T[0-9]+ / { print $2; n++ } END { exit n > 0 }' "$tmp/blocks" \
        || fail "a crate block in the notices has no license or text"
    grep '^Text: ' "$tmp/blocks" | grep -oE 'T[0-9]{4} \(' | cut -d' ' -f1 | LC_ALL=C sort -u > "$tmp/refs"
    sed -n 's/^=== \(T[0-9]*\)$/\1/p' "$notices" | LC_ALL=C sort -u > "$tmp/texts"
    [ -z "$(LC_ALL=C comm -23 "$tmp/refs" "$tmp/texts")" ] || fail "a text named in the notices is not in the file"
    sed -n '/^=== NVIDIA notice begin$/,/^=== NVIDIA notice end$/p' "$notices" | sed '1d;$d' > "$tmp/nvidia"
    cmp -s "$tmp/nvidia" crates/puron-testkit/fixtures/nvml/notice.txt || fail "the notices do not hold the NVIDIA notice"
fi
# No file of the package may hold this machine's home path, and no binary
# any path under /home. The terms are searched only in the strings of a
# binary's read-only data, at least 6 characters long, because short
# words occur by chance in machine code.
grep -rqF "$HOME/" "$tmp/x/usr" && fail "the package holds this home path"
for b in puron-applet puron-settings; do
    strings -a "$tmp/x/usr/bin/$b" > "$tmp/s" || fail "strings $b"
    grep -q '/home/' "$tmp/s" && fail "$b holds a home path: $(grep -m 1 '/home/' "$tmp/s" | cut -c1-120)"
    if [ -n "${PURON_TERMS:-}" ]; then
        grep -vE '^(#|$)' "$PURON_TERMS" > "$tmp/terms" || fail "no terms in $PURON_TERMS"
        objcopy -O binary --only-section=.rodata "$tmp/x/usr/bin/$b" "$tmp/r" || fail "objcopy $b"
        strings -a -n 6 "$tmp/r" > "$tmp/ro"
        if [ -n "${PURON_TERMS_SKIP:-}" ]; then
            grep -Fxvf "$PURON_TERMS_SKIP" "$tmp/ro" > "$tmp/ro2"; mv "$tmp/ro2" "$tmp/ro"
        fi
        while IFS="$(printf '\t')" read -r flags re; do
            case $flags in i) o=-i ;; *) o= ;; esac
            grep -qE $o -- "$re" "$tmp/ro" && fail "$b holds a private term: $re"
        done < "$tmp/terms"
    fi
done
if [ "$bad" = 0 ]; then echo "check-deb: ok ($deb)"; exit 0; fi
exit 1
