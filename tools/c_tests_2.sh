#!/bin/sh
# Runs cargo deny check licenses on the workspace with deny.toml and
# exits with its exit code (0 when every license is allowed).
# Usage: tools/c_tests_2.sh   (make check gives it 600 s; output to
# $PURON_DENY_OUT, default ${CARGO_TARGET_DIR:-target}/check/deny.txt)
cd "$(dirname "$0")/.." || exit 2
out=${PURON_DENY_OUT:-${CARGO_TARGET_DIR:-target}/check/deny.txt}
mkdir -p "$(dirname "$out")" || exit 2
RUSTUP_AUTO_INSTALL=0 "$HOME/.cargo/bin/cargo" deny check licenses > "$out" 2>&1
rc=$?
echo "C-tests-2: cargo deny exit $rc"
exit $rc
