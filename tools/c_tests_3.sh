#!/bin/sh
# Runs cargo deny check licenses on puron-engine (and so puron-core) with
# deny-core.toml, which is deny.toml without the GPL-3.0-only exception
# and without the clarify entries, and exits with its exit code.
# Usage: tools/c_tests_3.sh   (make check gives it 600 s; output to
# $PURON_DENY_CORE_OUT, default
# ${CARGO_TARGET_DIR:-target}/check/deny-core.txt)
cd "$(dirname "$0")/.." || exit 2
out=${PURON_DENY_CORE_OUT:-${CARGO_TARGET_DIR:-target}/check/deny-core.txt}
mkdir -p "$(dirname "$out")" || exit 2
RUSTUP_AUTO_INSTALL=0 "$HOME/.cargo/bin/cargo" deny \
    --manifest-path crates/puron-engine/Cargo.toml --config deny-core.toml \
    check licenses > "$out" 2>&1
rc=$?
echo "C-tests-3: cargo deny exit $rc"
exit $rc
