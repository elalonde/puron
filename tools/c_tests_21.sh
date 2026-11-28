#!/bin/sh
# Checks that the release notes of the AppStream metainfo come from
# CHANGELOG.md and from nowhere else, and that the newest release is the
# version in Cargo.toml. tools/metainfo-releases.py writes the
# <releases> element from the changelog; a hand edit of the element, or
# a changelog section that was not carried over, would show different
# notes in the software store than in the changelog. The script runs
# the generator's own check, compares its output with the element in
# the file, and then checks the result independently: one release per
# changelog heading with the heading's version and the heading's date at
# 12:00:00Z (the time keeps the software store from showing the day
# before west of UTC), no release with a date alone or a timestamp, the
# newest equal to the workspace version, and three releases whose text shows
# the conversion rules (a heading line becomes a paragraph, a list
# becomes a list, backticks and the scheme of web addresses are removed,
# since AppStream refuses links in a description).
# Exits 0, or 1 naming the first failed check.
# Usage: tools/c_tests_21.sh
cd "$(dirname "$0")/.." || exit 2
fail() { echo "C-tests-21: FAIL $*"; exit 1; }
python3 --version > /dev/null 2>&1 || { echo "HOST NEED python3"; exit 1; }
meta=data/io.github.elalonde.Puron.metainfo.xml
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
python3 -I tools/metainfo-releases.py --check > "$tmp/check.txt" 2>&1 \
    || { cat "$tmp/check.txt"; fail "tools/metainfo-releases.py --check"; }
python3 -I tools/metainfo-releases.py --print > "$tmp/print.xml" 2> "$tmp/print.err" \
    || { cat "$tmp/print.err"; fail "tools/metainfo-releases.py --print"; }
sed -n '/^ *<releases>$/,/^ *<\/releases>$/p' "$meta" > "$tmp/file.xml"
cmp -s "$tmp/print.xml" "$tmp/file.xml" || fail "the output of --print differs from the <releases> element of $meta"
n=$(grep -c '^## \[' CHANGELOG.md)
r=$(grep -c '<release version=' "$meta")
[ "$n" = "$r" ] || fail "$r release elements, but CHANGELOG.md has $n release headings"
sed -n 's/^## \[\([^]]*\)\] - \(.*\)$/\1 \2T12:00:00Z/p' CHANGELOG.md > "$tmp/headings"
sed -n 's/^ *<release version="\([^"]*\)" date="\([^"]*\)">$/\1 \2/p' "$meta" > "$tmp/releases"
cmp -s "$tmp/headings" "$tmp/releases" \
    || fail "the versions and dates of the releases differ from the changelog headings: $(diff "$tmp/headings" "$tmp/releases" | grep '^[<>]' | head -n 2 | tr '\n' ' ')"
ws=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
first=$(head -n 1 "$tmp/releases" | cut -d' ' -f1)
[ "$first" = "$ws" ] || fail "the newest release is $first, the workspace version is $ws"
python3 -I - "$meta" <<'EOF' || exit 1
import re
import sys
import xml.etree.ElementTree as ET


def fail(msg):
    print(f"C-tests-21: FAIL {msg}")
    sys.exit(1)


root = ET.parse(sys.argv[1]).getroot()

# Every release, however its attributes are written, carries the noon UTC
# date-time and no timestamp.
for r in root.findall("releases/release"):
    version, date = r.get("version"), r.get("date") or ""
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T12:00:00Z", date):
        fail(f"release {version} has the date {date!r}, not YYYY-MM-DDT12:00:00Z")
    if r.get("timestamp") is not None:
        fail(f"release {version} has a timestamp")


def release(version):
    r = root.find(f"releases/release[@version='{version}']")
    if r is None:
        fail(f"no release {version}")
    return r


desc = release("1.3.1").find("description")
# Whitespace between the tags is formatting, not text.
got = [(c.tag, (c.text or "").strip(), [(li.tag, li.text) for li in c]) for c in desc]
want = [("p", "Changed", []),
        ("ul", "", [("li", "Third-party libraries are updated to their latest compatible "
                           "releases, bringing upstream bug fixes.")])]
if got != want:
    fail(f"release 1.3.1 has {got}, want {want}")

text = "".join(release("1.2.0").itertext())
if "elalonde.dev/projects/puron/" not in text:
    fail("release 1.2.0 does not hold elalonde.dev/projects/puron/")
if "://" in text:
    fail("release 1.2.0 holds a web address scheme (://)")

text = "".join(release("1.0.0").itertext())
if "puron-settings" not in text:
    fail("release 1.0.0 does not hold puron-settings")
if "`" in text:
    fail("release 1.0.0 holds a backtick")
EOF
echo "C-tests-21: ok"
exit 0
