#!/bin/sh
# Runs the read-only test of each adapter (it checks that a read opens
# files for reading only and writes nothing) and c_src_registry_6 (which
# checks the table below against the provider table), each as
# cargo test -p puron-engine -- --exact <name>. Exits 0 only when all six
# exit 0 and each ran its one test.
# Usage: tools/read-only-run.sh   (output to read-only-run.txt under
# $PURON_CHECK_OUT, default ${CARGO_TARGET_DIR:-target}/check)
#
# One line per adapter, in provider order; c_src_registry_6 reads these
# lines.
# provider=provider_lm_sensors test=c_src_lmsensors_10
# provider=provider_liquidctl test=c_src_liquidctl_2
# provider=provider_nvidia test=c_src_nvidia_9
# provider=provider_udisks2 test=c_src_udisks2_10
# provider=provider_nvme test=c_src_nvme_10
cd "$(dirname "$0")/.." || exit 2
out=${PURON_CHECK_OUT:-${CARGO_TARGET_DIR:-target}/check}/read-only-run.txt
mkdir -p "$(dirname "$out")" || exit 2
: > "$out"
names="$(sed -n 's/^# provider=[a-z0-9_]* test=\([a-z0-9_]*\)$/\1/p' tools/read-only-run.sh) c_src_registry_6"
fail=0
tmp=$(mktemp) || exit 2
trap 'rm -f "$tmp"' EXIT
for t in $names; do
    cargo test -p puron-engine --locked -- --exact "$t" > "$tmp" 2>&1
    rc=$?
    # A name that matches no test runs nothing and exits 0: require it ran.
    if [ "$rc" = 0 ] && ! grep -q "^test $t \.\.\. ok$" "$tmp"; then rc=3; fi
    { echo "== $t exit $rc"; cat "$tmp"; } >> "$out"
    echo "$t exit $rc"
    [ "$rc" = 0 ] || fail=1
done
exit $fail
