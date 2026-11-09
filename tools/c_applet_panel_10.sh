#!/bin/sh
# Checks the icons that puron-applet uses. Each line of
# crates/puron-testkit/fixtures/icons/names.tsv names one icon in three
# tab-separated fields: its origin (`puron` for an icon this repository
# ships, `cosmic` for one of the COSMIC icon theme), its context
# directory, and its name. Every name must end in `-symbolic`, and every
# `"...-symbolic"` literal in puron-applet's source must be listed.
# Each Puron icon must be a file under data/icons with
# viewBox="0 0 16 16" and one fill value other than `none`, and make
# install-icons into a temporary root must put it under
# usr/share/icons/hicolor/scalable/status/. Each COSMIC icon must be a
# file under /usr/share/icons/Cosmic/scalable/<context>/, so the COSMIC
# icon theme must be installed.
# Usage: tools/c_applet_panel_10.sh
cd "$(dirname "$0")/.." || exit 2
tsv=crates/puron-testkit/fixtures/icons/names.tsv
dir=data/icons/hicolor/scalable/status
bad=0
fail() { echo "C-applet.panel-10: FAIL $*"; bad=1; }
if ! test -d /usr/share/icons/Cosmic; then
    echo "HOST NEED COSMIC icon theme at /usr/share/icons/Cosmic"
    exit 1
fi
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
n=$(cut -f3 "$tsv" | grep -vc -- '-symbolic$')
echo "names not ending in -symbolic: $n"
[ "$n" = 0 ] || fail "a name without -symbolic"
cut -f3 "$tsv" | LC_ALL=C sort -u > "$tmp/names"
grep -rhoE '"[a-z0-9-]+-symbolic"' crates/puron-applet/src | tr -d '"' \
    | LC_ALL=C sort -u > "$tmp/code"
[ -s "$tmp/code" ] || fail "no icon name in crates/puron-applet/src"
for x in $(LC_ALL=C comm -23 "$tmp/code" "$tmp/names"); do fail "source name $x not in $tsv"; done
make -s install-icons DESTDIR="$tmp/root" > "$tmp/install.txt" 2>&1 \
    || { cat "$tmp/install.txt"; fail "install step"; }
while IFS="$(printf '\t')" read -r src ctx name; do
    case $src in
    puron)
        f=$dir/$name.svg
        test -f "$f" || { fail "missing $f"; continue; }
        grep -q 'viewBox="0 0 16 16"' "$f" || fail "$f: viewBox"
        fills=$(grep -oE 'fill="[^"]*"' "$f" | grep -v 'fill="none"' | LC_ALL=C sort -u | wc -l)
        [ "$fills" = 1 ] || fail "$f: $fills fill values"
        test -f "$tmp/root/usr/share/icons/hicolor/scalable/status/$name.svg" \
            || fail "not installed: $name"
        ;;
    cosmic)
        test -f "/usr/share/icons/Cosmic/scalable/$ctx/$name.svg" \
            || fail "missing /usr/share/icons/Cosmic/scalable/$ctx/$name.svg"
        ;;
    *) fail "source $src" ;;
    esac
done < "$tsv"
if [ "$bad" = 0 ]; then echo "C-applet.panel-10: ok"; exit 0; fi
exit 1
