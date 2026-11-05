# Puron

Puron is a panel applet for the COSMIC desktop that displays hardware
sensor readings in the COSMIC Panel and Dock.

The goal of Puron is to read and display any sensor that the system
hardware makes available. This includes:
- individual hardware component temperatures (CPU, SATA, GPU, etc.)
- fan speeds
- hardware voltages
- liquid cooling statistics

## Build

You need rustup (the toolchain version is pinned in
`rust-toolchain.toml`), clang, `libsensors-dev` and the build packages
of libcosmic (among them `libxkbcommon-dev` and `libwayland-dev`).

    make install-bin DESTDIR=/tmp/puron-root   # or: cargo build --release

## Test

    make setup    # once per clone: enables the commit-msg hook
    make check

`make check` runs rustfmt, the build, the whole test suite and the
script checks under `tools/`. The test suite also needs umockdev,
dbus-daemon, jq, journald, cargo-deny, the locales `tr_TR.UTF-8` and
`de_DE.UTF-8` and the COSMIC icon theme; `tools/check-test-host.sh`
lists what is missing. A test whose host need is missing fails and
names the need.

## Install

    make deb      # also needs python3 and the crates of Cargo.lock
    sudo apt install ./target/deb/puron_1.0.0_amd64.deb

Then add Puron to a panel in COSMIC Settings, among the panel applets.
`make install PREFIX=/usr/local` installs without a package.

## Run-time dependencies

- `libsensors5` (required).
- Optional, each for one provider: the NVIDIA driver
  (`libnvidia-ml.so.1`), the udisks2 service (SATA drives), and the
  `liquidctl` program.

NVMe temperatures come from sysfs and need nothing more.

## Known limits

- On a computer with two NVIDIA GPUs where one is asleep, the sleeping
  GPU may be woken at each update. The NVIDIA provider can be switched
  off in the settings.
- liquidctl support is tested against recorded output, not against real
  devices.

## License

Puron's source files are MIT; see `LICENSE`.

The `puron-applet` binary links `cosmic-panel-config` and
`xdg-shell-wrapper-config` (GPL-3.0-only) and is therefore distributed
as a GPL-3.0 combined work; see `data/licenses/GPL-3.0.txt`. It also
links the libcosmic crates and `option-ext` (MPL-2.0), `udisks2`
(LGPL-2.1) and crates under permissive licenses.

The `puron-settings` binary is MIT and links the libcosmic crates and
`option-ext` (MPL-2.0) and crates under permissive licenses.

The package ships the GPL-3.0 text, `SOURCE-OFFER` and
`THIRD-PARTY-NOTICES.txt` in `/usr/share/doc/puron/`. The notices file
lists every crate in `Cargo.lock` with its license and notices.

The complete corresponding source of the binaries is the release asset
`puron-1.0.0-source.tar.xz` of the tag `v1.0.0`: Puron's full source
with the locked dependencies vendored and the cargo config for an
offline build.

    make source-archive
    sh tools/check-source-archive.sh

## History
Puron started as a clean-room re-implementation of the GNOME Shell extension Freon, 
to avoid license issues. (Puron is the refrigerant which replaced Freon after Freon
was banned due to its contribution to ozone depletion). The direction of the project
has since moved on from its original intent in order to support more modern environments.
