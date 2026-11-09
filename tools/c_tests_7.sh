#!/bin/sh
# Checks that a missing locale fails loudly rather than passing. With
# LOCPATH set to an empty directory, glibc loads no installed locale;
# then the locale tests (c_core_order_10 in puron-core, c_i18n_12 in
# puron-applet and in puron-settings) and tools/check-test-host.sh must
# each exit non-zero. Each test must print HOST NEED tr_TR.UTF-8 on
# standard error, and the host check must report both locales MISSING.
# Usage: tools/c_tests_7.sh
cd "$(dirname "$0")/.." || exit 2
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/loc"
rc=0
run() {    # run <name> <command...>: exit code must be non-zero
    name=$1; shift
    # Keep stderr in its own file: otherwise libtest's "test <name> ... "
    # of a parallel test, printed on stdout before its newline, could
    # end up on the same line as the need.
    LOCPATH=$tmp/loc "$@" > "$tmp/$name.out" 2> "$tmp/$name.err"; r=$?
    echo "C-tests-7: $name exit $r"
    [ "$r" != 0 ] || { echo "C-tests-7: $name passed with no locale"; rc=1; }
}
need() {    # need <name>: the test printed the need line
    n=$(grep -cx 'HOST NEED tr_TR.UTF-8' "$tmp/$1.err")
    echo "C-tests-7: $1 HOST NEED lines $n"
    [ "$n" -ge 1 ] || { echo "C-tests-7: $1 did not name the need"; tail -n 5 "$tmp/$1.out" "$tmp/$1.err"; rc=1; }
}
run order cargo test -p puron-core --test c_core_order c_core_order_10 -- --nocapture
need order
run applet cargo test -p puron-applet --test c_i18n_12 -- --nocapture
need applet
run settings cargo test -p puron-settings --test c_i18n_12 -- --nocapture
need settings
run host sh tools/check-test-host.sh
n=$(cat "$tmp/host.out" "$tmp/host.err" | grep -cE '^MISSING locale (tr_TR|de_DE)\.UTF-8$')
echo "C-tests-7: host MISSING locale lines $n"
[ "$n" = 2 ] || rc=1
exit $rc
