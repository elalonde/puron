#!/bin/sh
# Checks that puron-settings depends on none of puron-engine, lm-sensors,
# nvml-wrapper and udisks2. The count printed by grep -c must be 0
# (grep -c exits 1 on no match, so the result is the printed count).
# Usage: tools/c_settings_window_15.sh   (make check gives it 120 s)
cd "$(dirname "$0")/.." || exit 2
export RUSTUP_AUTO_INSTALL=0
tree=$(cargo tree -p puron-settings -e normal --prefix none --offline --locked) || exit 2
n=$(printf '%s\n' "$tree" | grep -c -E '^(puron-engine|lm-sensors|nvml-wrapper|udisks2) ')
echo "C-settings.window-15: count $n"
[ "$n" = 0 ]
