#!/bin/sh
# Lint A: no rule IDs, spec or process references, home paths, IP
# addresses or citations of another project's files in comments and
# docs. The patterns are in tools/lint-patterns. Comments are taken from
# Rust files (// and block comments) and from shell, Makefile, TOML, YAML,
# Fluent and desktop files (#); docs are README.md, CHANGELOG.md, LICENSE and debian/.
# Usage: tools/lint-a.sh [file...]; with no file, every file git knows
# (tracked or not ignored). Exits 1 and prints file:line for each hit.
cd "$(dirname "$0")/.." || exit 2
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
grep -vE '^(#|$)' tools/lint-patterns | sed "s/<SECTION-SIGN>/$(printf '\302\247')/g" > "$tmp/pat" || exit 2
if [ $# -gt 0 ]; then
    printf '%s\n' "$@" > "$tmp/files"
else
    git ls-files --cached --others --exclude-standard > "$tmp/files" || exit 2
fi
# comments <kind> <file>: prints "<line>:<comment text>".
comments() {
    case $1 in
    rust)
        awk '{
            t = $0; out = ""
            if (inb) {
                e = index(t, "*/")
                if (e) { out = substr(t, 1, e + 1); t = substr(t, e + 2); inb = 0 }
                else { out = t; t = "" }
            }
            while (t != "") {
                s = index(t, "//"); b = index(t, "/*")
                if (s && (!b || s < b)) { out = out substr(t, s); break }
                if (b) {
                    r = substr(t, b + 2); e = index(r, "*/")
                    if (e) { out = out substr(t, b, e + 3); t = substr(r, e + 2) }
                    else { out = out substr(t, b); inb = 1; t = "" }
                } else break
            }
            if (out != "") print NR ": " out
        }' "$2" ;;
    hash)
        awk '{
            if ($0 ~ /^[ \t]*#/) { print NR ": " $0; next }
            if (match($0, /[ \t]#/)) print NR ": " substr($0, RSTART + 1)
        }' "$2" ;;
    doc)
        awk '{ print NR ": " $0 }' "$2" ;;
    esac
}
bad=0
while IFS= read -r f; do
    [ -f "$f" ] || continue
    case $f in
    crates/*/fixtures/*.sh) k=hash ;;
    crates/*/fixtures/*) continue ;;
    tools/lint-patterns) continue ;;
    *.rs) k=rust ;;
    *.sh|Makefile|*.toml|*.yml|*.ftl|*.desktop.in|.gitattributes|.gitignore|.githooks/*) k=hash ;;
    README.md|CHANGELOG.md|LICENSE|debian/*) k=doc ;;
    *) continue ;;
    esac
    comments "$k" "$f" > "$tmp/c"
    while IFS="$(printf '\t')" read -r class re; do
        grep -E -- "^[0-9]+:.*($re)" "$tmp/c" | while IFS= read -r hit; do
            echo "lint-a: $f:${hit%%:*}: $class: ${hit#*: }"
        done
    done < "$tmp/pat" > "$tmp/h"
    if [ -s "$tmp/h" ]; then cat "$tmp/h"; bad=1; fi
done < "$tmp/files"
[ "$bad" = 0 ] && echo "lint-a: ok"
exit $bad
