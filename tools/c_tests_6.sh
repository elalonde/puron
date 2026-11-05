#!/bin/sh
# Checks that every #[test] function in the workspace calls the time
# limit helper puron_testkit::limit( in its body. Prints each test
# without it; exits 1 if there is one, or if no test is found.
# Usage: tools/c_tests_6.sh
cd "$(dirname "$0")/.." || exit 2
find crates -name target -prune -o -name '*.rs' -type f -print | sort \
| xargs awk '
FNR == 1 { want = 0; inbody = 0 }
# A #[test] attribute: the next fn is a test.
/^[ \t]*#\[test\]/ { want = 1; next }
want && !inbody && /fn [A-Za-z0-9_]+/ {
    name = $0; sub(/.*fn /, "", name); sub(/[^A-Za-z0-9_].*/, "", name)
    inbody = 1; depth = 0; started = 0; found = 0; want = 0; tests++
}
inbody {
    if (index($0, "puron_testkit::limit(")) found = 1
    line = $0
    o = gsub(/\{/, "{", line); c = gsub(/\}/, "}", line)
    depth += o - c; if (o) started = 1
    if (started && depth <= 0) {
        inbody = 0
        if (!found) { print "C-tests-6: no limit helper: " FILENAME ": " name; bad++ }
    }
}
END {
    print "C-tests-6: " tests + 0 " tests, " bad + 0 " without the helper"
    exit (bad > 0 || tests == 0) ? 1 : 0
}'
