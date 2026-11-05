#!/bin/sh
# Checks the install step. Runs make install DESTDIR=<tmp>
# PREFIX=/opt/puron-c15 (a release build in the workspace's target
# directory), then checks under <tmp> the desktop entry (D in the
# messages), the installed binary and Puron's four icon files, and that
# nothing was put in <tmp>/usr. The release build has no time limit; the
# checks after it run under timeout -k 5 120 (this script again with
# --after <tmp>). Exits 0, or 1 naming the first failed check (124 at the
# time limit).
# Usage: tools/c_applet_panel_15.sh
cd "$(dirname "$0")/.." || exit 2
prefix=/opt/puron-c15
fail() { echo "C-applet.panel-15: FAIL $*"; exit 1; }
if [ "${1-}" = --after ]; then
    tmp=$2
    d=$tmp$prefix/share/applications/io.github.elalonde.Puron.desktop
    test -f "$d" || fail "test -f $d"
    [ "$(sed -n 1p "$d")" = "[Desktop Entry]" ] || fail "line 1 of D is not [Desktop Entry]"
    # Each key must appear once, with exactly this value.
    while IFS='=' read -r k v; do
        n=$(grep -c "^$k=" "$d")
        [ "$n" = 1 ] || fail "grep -c '^$k=' D prints $n"
        n=$(grep -cx "$k=$v" "$d")
        [ "$n" = 1 ] || fail "grep -cx '$k=$v' D prints $n"
    done <<EOF
Name=Puron
Type=Application
Exec=$prefix/bin/puron-applet
Terminal=false
Categories=COSMIC;
Keywords=COSMIC;Iced;
Icon=puron-temperature-symbolic
StartupNotify=true
NoDisplay=true
X-CosmicApplet=true
X-CosmicHoverPopup=Auto
EOF
    n=$(wc -l < "$d")
    [ "$n" = 12 ] || fail "wc -l < D prints $n"
    test -x "$tmp$prefix/bin/puron-applet" || fail "test -x $tmp$prefix/bin/puron-applet"
    names=$(awk -F '\t' '$1 == "puron" { print $3 }' crates/puron-testkit/fixtures/icons/names.tsv)
    [ "$(echo "$names" | wc -l)" = 4 ] || fail "not four Puron icon names"
    for name in $names; do
        f=$tmp$prefix/share/icons/hicolor/scalable/status/$name.svg
        test -f "$f" || fail "test -f $f"
    done
    test ! -e "$tmp/usr" || fail "$tmp/usr exists"
    echo "C-applet.panel-15: ok"
    exit 0
fi
tmp=$(mktemp -d) || exit 2
log=$(mktemp) || exit 2
trap 'rm -rf "$tmp" "$log"' EXIT
make -s install DESTDIR="$tmp" PREFIX=$prefix > "$log" 2>&1 \
    || { tail -n 20 "$log"; fail "make install"; }
timeout -k 5 120 sh tools/c_applet_panel_15.sh --after "$tmp"
