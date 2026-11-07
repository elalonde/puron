# Changelog

The notable changes of each Puron release. Versions follow
[Semantic Versioning](https://semver.org/), and the format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.1.0] - 2026-11-07

### Changed

- The popup shows everything at once, in the sections Temperature, Fan, Voltage and Power. Nothing opens or closes
  any more, so the arrows and the "Group Items" settings are gone.
- Temperatures are listed by device: the CPU first, with every CPU reading under it, then GPUs, drives and other chips,
  each under its name, or the label you set in lm-sensors on a drive's "Composite" sensor.
- A device with several readings also has a "Maximum" line, the highest of them at each poll, and marks one line
  "(default)": the CPU's package reading, a drive's composite reading, otherwise the first.
- A click on a line checks it, and a checked line shows on the panel; several can be checked, and the panel shows them
  in popup order. A new install checks each "(default)" line. The Maximum that version 1.0.0 pinned moves to the CPU's
  default line, once.
- With lm-sensors and the "nvme-cli" switch both on, a drive no longer shows the same reading twice: the lm-sensors
  line stays, and a pin on the other one moves to it.
- The check mark is a small fixed size with a gap before the text, and section headings are drawn stronger.
- Every line stays on one line. The popup grows to fit, and text over 44 characters is shortened with "…". A popup
  taller than its limit scrolls.

Changes to the repository only, not to the shipped program:

- The checks run on GitHub Actions, and the README shows their status.
- This changelog.

## [1.0.0] - 2026-11-05

First release.

- A panel applet for the COSMIC desktop that displays hardware sensor
  readings in the COSMIC Panel and Dock: component temperatures, fan
  speeds, voltages and liquid cooling statistics.
- A settings application, `puron-settings`.
- A Debian package for amd64, `puron_1.0.0_amd64.deb`.
- The complete corresponding source as a release asset,
  `puron-1.0.0-source.tar.xz`, with the locked dependencies vendored for
  an offline build.
