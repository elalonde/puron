#!/bin/sh
# Checks that every process creation in crates/puron-engine/src is in
# the liquidctl runner (liquidctl/runner.rs), and that no string literal
# there names a tool Puron does not use (smartctl, hddtemp, aticonfig,
# optirun, ipmi). Exits 1 naming each hit.
# Usage: tools/c_src_registry_7.sh
cd "$(dirname "$0")/.." || exit 2
tree=crates/puron-engine/src
runner=$tree/liquidctl/runner.rs
[ -d "$tree" ] || { echo "C-src.registry-7: no $tree"; exit 2; }
bad=0
hits=$(grep -rnE 'Command::new|\<fork\>|\<exec(l|lp|le|v|vp|vpe|ve)?\>|posix_spawn' "$tree")
rc=$?
[ "$rc" -le 1 ] || exit 2
if [ -n "$hits" ]; then
    out=$(printf '%s\n' "$hits" | grep -v "^$runner:")
    if [ -n "$out" ]; then
        printf 'C-src.registry-7: process creation outside the runner: %s\n' "$out"
        bad=1
    fi
fi
grep -rniE '"[^"]*(smartctl|hddtemp|aticonfig|optirun|ipmi)[^"]*"' "$tree"
case $? in
0) echo "C-src.registry-7: a removed tool named in a string literal"; bad=1 ;;
1) ;;
*) exit 2 ;;
esac
echo "C-src.registry-7: bad=$bad"
exit $bad
