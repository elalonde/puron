#!/bin/sh
# Checks the toolchain: rustc --version must report release 1.99.1
# (the exact line below), and rust-toolchain.toml must pin channel
# 1.99.1.
# Usage: tools/c_tests_1.sh   (from any directory; runs in the
# repository root)
# RUSTUP_AUTO_INSTALL=0 makes a channel that is not installed fail here
# instead of being downloaded.
cd "$(dirname "$0")/.." || exit 2
rc=0
v=$(RUSTUP_AUTO_INSTALL=0 rustc --version 2>&1)
n=$(printf '%s\n' "$v" | grep -cx 'rustc 1.99.1 (58cf8f9c3 2026-10-13)')
[ "$n" = 1 ] || { echo "C-tests-1: rustc --version: $v"; rc=1; }
c=$(grep -cx 'channel = "1.99.1"' rust-toolchain.toml)
[ "$c" = 1 ] || { echo "C-tests-1: channel line found $c times"; rc=1; }
exit $rc
