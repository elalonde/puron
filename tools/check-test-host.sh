#!/bin/sh
# Checks that this machine has what the build and the test suite need.
# Usage: tools/check-test-host.sh
# Prints "ok <need>" or "MISSING <need>" per need; exits 1 when any need
# is missing, naming each one.

missing=0
need() {    # need <name> <check command>
    if timeout -k 5 120 sh -c "$2" >/dev/null 2>&1; then
        echo "ok $1"
    else
        echo "MISSING $1"; missing=1
    fi
}

need "rustc 1.99.1 through rustup" \
    '~/.cargo/bin/rustc --version | grep -qx "rustc 1.99.1 (58cf8f9c3 2026-10-13)"'
need "cargo-deny 0.20.2" '~/.cargo/bin/cargo deny --version | grep -qx "cargo-deny 0.20.2"'
need "clang and libsensors-dev" 'dpkg-query -W clang libsensors-dev'
need "libcosmic build packages" 'pkg-config --modversion xkbcommon wayland-client'
need "libsensors5 and sensors" 'dpkg-query -W libsensors5 lm-sensors'
need "umockdev with the preload library" \
    'umockdev-run --version && test -e /usr/lib/x86_64-linux-gnu/libumockdev-preload.so.0'
need "dbus-daemon" 'dbus-daemon --version'
need "jq" 'jq --version'
need "journald storing priority 7" \
    'T=chk$$; echo x | systemd-cat -t $T -p debug; sleep 1; journalctl -t $T -o json --no-pager | jq -er .MESSAGE'
# A locale counts only when it is listed and also loads: with LOCPATH
# set, locale -a still lists the archive, but loading fails and the
# charmap falls back to ANSI_X3.4-1968.
need "locale tr_TR.UTF-8" \
    'locale -a | grep -qx "tr_TR\.utf8" && [ "$(LC_ALL=tr_TR.UTF-8 locale charmap 2>/dev/null)" = UTF-8 ]'
need "locale de_DE.UTF-8" \
    'locale -a | grep -qx "de_DE\.utf8" && [ "$(LC_ALL=de_DE.UTF-8 locale charmap 2>/dev/null)" = UTF-8 ]'
need "a test user who is not root" 'test "$(id -u)" -ne 0'
need "/bin/sh, mkfifo, setsid, pgrep, ps, /proc" \
    'test -x /bin/sh && command -v mkfifo setsid pgrep ps && test -d /proc/self'
need "git" 'git --version'
need "COSMIC icon theme" 'test -d /usr/share/icons/Cosmic'

exit $missing
