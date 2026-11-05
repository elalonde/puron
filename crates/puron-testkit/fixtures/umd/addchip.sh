# Adds the chip puron_new under $UMOCKDEV_DIR while umockdev-run runs.
set -eu
S=$UMOCKDEV_DIR/sys; D=$S/devices/platform/puron_new; H=$D/hwmon/hwmon6
mkdir -p $H
ln -s ../../bus/platform $D/subsystem; : > $D/uevent
ln -s ../../../puron_new $H/device
ln -s ../../../../../class/hwmon $H/subsystem
printf 'puron_new\n' > $H/name; printf '30000\n' > $H/temp1_input; : > $H/uevent
ln -s ../../devices/platform/puron_new/hwmon/hwmon6 $S/class/hwmon/hwmon6
