#!/usr/bin/env python3
"""Write the release notes of the AppStream metainfo from CHANGELOG.md.

The software stores show each release with its version, date and notes,
which they read from the <releases> element of
data/io.github.elalonde.Puron.metainfo.xml. CHANGELOG.md is the one place
where release notes are written; this script turns it into that element,
so the two can never disagree. Nobody edits the element by hand.

Usage: python3 -I tools/metainfo-releases.py --print | --write | --check

  --print  writes the <releases> element to standard output.
  --write  replaces the <releases> element of the metainfo with it, and
           sets the version in every screenshot address to the workspace
           version. Nothing else in the file changes.
  --check  exits 0 when --write would change nothing and the newest
           release is the workspace version; else it prints the first
           difference and exits 1.

Each heading "## [X.Y.Z] - YYYY-MM-DD" of the changelog gives one
<release>, newest first as in the file, dated YYYY-MM-DDT12:00:00Z. The
time is there because the COSMIC Store reads a date alone as midnight
UTC and shows it in local time, which west of UTC is the day before;
noon UTC is the same calendar day in nearly every time zone. The lines
up to the next "## " heading become its <description>:
  - "### <text>" becomes a paragraph of its own;
  - "- <text>" starts a list item, and the lines right after it that
    start with two spaces continue it; consecutive items, blank lines
    between them or not, form one list;
  - other lines that are not blank are paragraph text, joined until a
    blank line.
In every text, backticks and the scheme of web addresses ("https://",
"http://") are removed and Markdown links keep only their text, because
AppStream refuses web links in a description; the address stays
readable without its scheme.
"""

import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
METAINFO = ROOT / "data" / "io.github.elalonde.Puron.metainfo.xml"
CARGO = ROOT / "Cargo.toml"

HEADING = re.compile(r"^## \[(\d+\.\d+\.\d+)\] - (\d{4}-\d{2}-\d{2})$")
LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
SCREENSHOT = re.compile(
    r"(https://raw\.githubusercontent\.com/elalonde/puron/v)[^/]*(/docs/screenshots/)"
)

# The indentation of the element inside <component>, and of each level
# below it. The metainfo is written with two spaces per level.
INDENT = "  "


class Error(Exception):
    pass


def workspace_version():
    with open(CARGO, "rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def clean(text):
    text = text.replace("`", "")
    text = LINK.sub(r"\1", text)
    text = text.replace("https://", "").replace("http://", "")
    text = " ".join(text.split())
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def blocks(lines):
    """Turn the lines of one changelog section into a list of blocks.

    A block is ("p", text) or ("ul", [item text, ...]).
    """
    out = []
    para = None  # lines of the paragraph being read
    item = None  # lines of the list item being read

    def end_para():
        nonlocal para
        if para is not None:
            out.append(("p", " ".join(para)))
            para = None

    def end_item():
        nonlocal item
        if item is not None:
            # Items that follow one another, even across blank lines,
            # belong to the same list.
            if out and out[-1][0] == "ul":
                out[-1][1].append(" ".join(item))
            else:
                out.append(("ul", [" ".join(item)]))
            item = None

    previous_blank = True
    for line in lines:
        if line.strip() == "":
            end_para()
            end_item()
            previous_blank = True
            continue
        if line.startswith("### "):
            end_para()
            end_item()
            out.append(("p", line[4:]))
        elif line.startswith("- "):
            end_para()
            end_item()
            item = [line[2:]]
        elif line.startswith("  ") and item is not None and not previous_blank:
            item.append(line.strip())
        else:
            end_item()
            if para is None:
                para = []
            para.append(line)
        previous_blank = False
    end_para()
    end_item()
    return out


def sections():
    """Return (version, date, lines) for each release of the changelog."""
    result = []
    current = None
    for line in CHANGELOG.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            m = HEADING.match(line)
            current = [m.group(1), m.group(2), []] if m else None
            if current:
                result.append(current)
        elif current is not None:
            current[2].append(line)
    return result


def releases_element():
    lines = [INDENT + "<releases>"]
    for version, date, body in sections():
        texts = []
        for kind, value in blocks(body):
            if kind == "p":
                texts.append(("p", clean(value)))
            else:
                texts.append(("ul", [clean(v) for v in value]))
        if not any(v for _, v in texts):
            raise Error(f"release {version} has no text in {CHANGELOG.name}")
        i2, i3, i4, i5 = INDENT * 2, INDENT * 3, INDENT * 4, INDENT * 5
        lines.append(f'{i2}<release version="{version}" date="{date}T12:00:00Z">')
        lines.append(f"{i3}<description>")
        for kind, value in texts:
            if kind == "p":
                lines.append(f"{i4}<p>{value}</p>")
            else:
                lines.append(f"{i4}<ul>")
                lines.extend(f"{i5}<li>{v}</li>" for v in value)
                lines.append(f"{i4}</ul>")
        lines.append(f"{i3}</description>")
        lines.append(f"{i2}</release>")
    lines.append(INDENT + "</releases>")
    return lines


def written_metainfo(old):
    """Return the metainfo text that --write makes from the text old."""
    lines = old.splitlines()
    starts = [i for i, l in enumerate(lines) if l.strip() == "<releases>"]
    ends = [i for i, l in enumerate(lines) if l.strip() == "</releases>"]
    if len(starts) != 1 or len(ends) != 1 or ends[0] < starts[0]:
        raise Error(f"{METAINFO.name} needs exactly one <releases> element on lines of its own")
    new = lines[: starts[0]] + releases_element() + lines[ends[0] + 1 :]
    version = workspace_version()
    new = [SCREENSHOT.sub(rf"\g<1>{version}\g<2>", l) for l in new]
    return "\n".join(new) + "\n"


def check():
    old = METAINFO.read_text(encoding="utf-8")
    new = written_metainfo(old)
    if old != new:
        a, b = old.splitlines(), new.splitlines()
        for n in range(max(len(a), len(b))):
            x = a[n] if n < len(a) else "(end of file)"
            y = b[n] if n < len(b) else "(end of file)"
            if x != y:
                print(f"{METAINFO.name}:{n + 1}: is: {x.strip()}")
                print(f"{METAINFO.name}:{n + 1}: want: {y.strip()}")
                break
        return 1
    # On a line between two releases the changelog and the workspace both
    # still name the last release; a version commit moves both at once.
    first = sections()[0][0] if sections() else None
    version = workspace_version()
    if first != version:
        print(f"newest release in {CHANGELOG.name} is {first}, workspace version is {version}")
        return 1
    return 0


def main(argv):
    if len(argv) != 2 or argv[1] not in ("--print", "--write", "--check"):
        print("usage: python3 -I tools/metainfo-releases.py --print | --write | --check", file=sys.stderr)
        return 2
    try:
        if argv[1] == "--print":
            print("\n".join(releases_element()))
            return 0
        if argv[1] == "--write":
            METAINFO.write_text(written_metainfo(METAINFO.read_text(encoding="utf-8")), encoding="utf-8")
            return 0
        return check()
    except Error as e:
        print(f"metainfo-releases: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
