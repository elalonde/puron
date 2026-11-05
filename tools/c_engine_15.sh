#!/bin/sh
# Builds puron-release-check with the release profile and runs it. The
# binary makes a provider panic during a read and checks that the engine
# still reads after it, which needs the release profile to unwind on
# panic. The build has no time limit; the run has 30 s. Exits 0 when the
# build and the run exit 0 and the run prints its success line once.
# Usage: tools/c_engine_15.sh   (output under $PURON_CHECK_OUT, default
# ${CARGO_TARGET_DIR:-target}/check)
cd "$(dirname "$0")/.." || exit 2
out=${PURON_CHECK_OUT:-${CARGO_TARGET_DIR:-target}/check}
mkdir -p "$out"
cargo build --release --locked -p puron-testkit --bin puron-release-check \
    > "$out/release-build.txt" 2>&1
build_rc=$?
echo "build_rc=$build_rc"
[ "$build_rc" -eq 0 ] || exit 1
target=${CARGO_TARGET_DIR:-target}
timeout -k 5 30 "$target/release/puron-release-check" \
    > "$out/release-run.txt" 2>&1
run_rc=$?
echo "run_rc=$run_rc"
n=$(grep -cx 'C-engine-15 read-after-panic' "$out/release-run.txt")
echo "lines=$n"
[ "$run_rc" -eq 0 ] && [ "$n" -eq 1 ]
