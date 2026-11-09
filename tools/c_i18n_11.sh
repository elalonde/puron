#!/bin/sh
# Checks that every message ID of the English translation file appears
# in an fl! call in the sources of puron-applet or puron-settings, so
# that no text goes unused.
# Prints each unused ID; exits 1 if there is one.
# Usage: tools/c_i18n_11.sh
cd "$(dirname "$0")/.." || exit 2
ids=$(sed -n 's/^\([a-z][a-z0-9-]*\) =.*/\1/p' i18n/en/puron.ftl)
[ -n "$ids" ] || { echo "C-i18n-11: no ID in i18n/en/puron.ftl"; exit 1; }
calls=$(grep -rhoE 'fl!\([^"]*"[a-z0-9-]+"' crates/puron-applet/src crates/puron-settings/src \
    | sed 's/.*"\([a-z0-9-]*\)"$/\1/' | sort -u)
unused=0
for id in $ids; do
    printf '%s\n' "$calls" | grep -qxF "$id" && continue
    echo "C-i18n-11: unused: $id"; unused=$((unused + 1))
done
echo "C-i18n-11: unused IDs: $unused"
[ "$unused" = 0 ]
