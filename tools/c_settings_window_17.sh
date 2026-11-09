#!/bin/sh
# Checks the settings program's window: runs the test
# c_settings_window_17 (title, application ID, sizes), then checks that
# the feature tree of puron-settings has the libcosmic feature
# "single-instance" and no feature "applet".
# Usage: tools/c_settings_window_17.sh   (make check gives it 120 s)
cd "$(dirname "$0")/.." || exit 2
export RUSTUP_AUTO_INSTALL=0
cargo test -q -p puron-settings --test c_settings_window -- --exact c_settings_window_17 \
    || { echo "C-settings.window-17: window settings differ"; exit 1; }
tree=$(cargo tree -p puron-settings -e features --offline --locked) || exit 2
single=$(printf '%s\n' "$tree" | grep -c 'libcosmic feature "single-instance"')
applet=$(printf '%s\n' "$tree" | grep -c 'feature "applet"')
echo "C-settings.window-17: single-instance $single, applet $applet"
[ "$single" -ge 1 ] && [ "$applet" = 0 ]
