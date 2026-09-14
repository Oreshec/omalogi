# Changelog

All notable changes are listed here. The project follows
[Semantic Versioning](https://semver.org).

## Unreleased

### Added

- `omalogi info`, `omalogi profiles` and `omalogi backup`: device, firmware, DPI and
  report rate; onboard profiles with DPI stages and button bindings; a full backup of
  profile memory. Every command supports `--json`.
- `omalogi profiles activate <N>` switches the active onboard profile and reads it back.
- `omalogi profiles edit <N>` changes DPI stages, the default and DPI-shift stages, the
  report rate and button and G-Shift bindings, with `--dry-run`. Each write is preceded
  by an automatic backup, verified by reading it back, and rolled back on a mismatch.
- `omalogi restore <FILE>` writes profile memory back from a backup, with `--dry-run`.
- `omalogi daemon` switches profiles per app and per monitor from rules in
  `~/.config/omalogi/config.toml`, and publishes its state for the shell plugin.
- Omarchy shell plugin: a G HUB-style editor with the mouse, a card and leader line per
  button, a grouped and searchable action picker, a shortcut recorder, a G-Shift layer,
  DPI stages and report rate. Changes collect until **Apply to mouse**, which previews,
  confirms and does a verified write. Opens on a given profile, view or button. Plus a
  bar indicator for the active profile.
- Keyboard shortcuts can use any standard key, including punctuation, navigation keys
  and F13–F24.
- Edits and restores that change the profile in use take effect immediately: the mouse
  only loads a profile when switching to it, so Omalogi switches away and back after the
  verified write. Writes and restores report whether the change is in use now, applies
  once the profile is activated, or could not be loaded.
- `omalogi setup` installs the shell plugin built into the binary, puts the indicator on
  the bar, enables the daemon and checks device access, for the current user and without
  root. `--dry-run`, `--no-bar` and `--no-daemon`.
- udev rule granting the active session access to the G502 X's HID++ interface only.
- systemd user unit for the daemon.
- Arch Linux PKGBUILD (`packaging/aur/omalogi`).
- `omalogi picture` downloads the mouse's render and button positions once from
  assets.openlogi.org, verified by checksum and cached; the overlay shows the render
  with a badge on each button, linked to the binding table.

### Supported devices

- Logitech G502 X, wired (046d:c099), tested on real hardware.
