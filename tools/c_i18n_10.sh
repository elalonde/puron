#!/bin/sh
# Checks that the source tree holds only the English translation file:
# find i18n -name '*.ftl' must print exactly i18n/en/puron.ftl.
# Usage: tools/c_i18n_10.sh
cd "$(dirname "$0")/.." || exit 2
got=$(find i18n -name '*.ftl' | LC_ALL=C sort)
printf '%s\n' "$got"
if [ "$got" = i18n/en/puron.ftl ]; then echo "C-i18n-10: ok"; exit 0; fi
echo "C-i18n-10: FAIL (expected exactly i18n/en/puron.ftl)"; exit 1
