# Changelog

All notable changes are listed here. The project follows
[Semantic Versioning](https://semver.org).

## Unreleased

## [0.3.3] - 2026-09-17

### Fixed

- Omalogi says when Omarchy's shell is running an older copy of the overlay than the
  installed files, and offers to restart the shell. The shell keeps a plugin's overlay
  loaded, so until now an update silently kept the previous version running.

## [0.3.2] - 2026-09-17

### Fixed

- Install helper, Update helper and Allow access close the overlay before opening the
  installer's terminal. The overlay covers the whole screen, so the terminal opened out
  of sight and could not be reached. The installer ends by saying to open Omalogi again.
- `install.sh` no longer hangs when GitHub's download host stalls: each download gives
  up on a connection after 15 seconds and on a transfer after 5 minutes, retries, and
  says what failed.

## [0.3.1] - 2026-09-17

### Added

- **Uninstall** in the overlay's header, also on the setup screen. It asks with Omarchy's
  confirm dialog, then runs the plugin's `uninstall.sh` in the floating terminal, where
  its plan and the sudo prompt stay visible. `uninstall.sh --yes` skips its own question
  for callers that already asked.

## [0.3.0] - 2026-09-17

### Added

- `omalogi profiles repair` rebuilds a profile directory that fails its checksum from its
  own entries, with `--dry-run`. It writes only the directory, and only when the entries
  are consistent and every profile they list passes its checksum; the write is backed up
  first and verified. The overlay offers the same repair, and `omalogi serve` gains
  `repair_directory` and a `kind` on errors it can act on (protocol 3). Reported with a
  byte-level analysis by @davidichung (#16).
- Mouse reports: one issue per model, listed in #15. A "Verify it" report is assigned to
  its author as the claim on the model, `/claim` and `/unclaim` take and return a claim
  on an existing issue, and a second report for the same model is pointed at the first.
  The overlay's **Help verify it** opens the form filled in.
- `uninstall.sh` reverses the installer: it stops the daemon and removes its unit, the
  helper, the udev rule and the plugin, after showing the plan and asking. `--dry-run`
  shows the plan only. Backups, rules and the mouse's profiles are kept.
- `docs/agents/verify-my-mouse.md`: a prompt that walks an owner and their coding agent
  through claiming a model, the self-test and the pull request that marks it verified.

### Fixed

- A profile directory that fails its checksum no longer blocks everything: `backup` saves
  such sectors as read and flags them, and `restore` works past them. The error says the
  profiles may be intact and names the repair, instead of blaming a mouse that never had
  profiles written (#16).
- `install.sh` updates an older udev rule instead of keeping it: a rule in `/usr/lib` is
  only trusted while it matches the release, since older rules miss newer mice.
- The setup screen no longer shows the editor's keyboard hints.
- The hardware self-test picks its DPI cases from each sensor's supported values and
  retries a live DPI read that times out right after a profile reload.

## [0.2.0] - 2026-09-14

### Added

- Install Omalogi as a native Omarchy plugin with
  `omarchy plugin add https://github.com/elberacasa/omalogi --enable`. When its helper is
  missing, too old, or cannot open the mouse, Omalogi says what is wrong and runs the
  plugin's own `install.sh` in Omarchy's terminal; nothing is ever downloaded and piped
  to a shell. `omarchy plugin update` keeps the plugin current: `omalogi setup` no
  longer writes into a plugin added this way, and `omalogi serve` reports the helper's
  version and protocol.
- Untested mice: every wired G-series mouse libratbag lists with onboard profiles (27
  models) is found and read. Omalogi reads and edits all five onboard profile formats
  libratbag shares one layout for. Editing an untested mouse needs a one-time
  acceptance per model and layout (`omalogi accept-untested`, or the overlay), and
  every write is still backed up and verified; `omalogi serve` reports the mouse's
  `support`. The udev rule covers the new models.
- Wireless mice: 16 untested wireless G-series models are found and edited through
  LIGHTSPEED, Bolt and Unifying receivers, using OpenLogi's device layer
  (`openlogi-hid`, `openlogi-core`) for receivers, inventory and routing. A mouse on a
  cable is used first; the udev rule covers the receivers. `omalogi picture` works for
  wireless mice too.
- `docs/agents/agent-guide.md` and task prompts in `docs/agents/` so contributors can work with the coding
  agent of their choice under the same rules.

### Changed

- The README uninstall steps start with `omarchy plugin remove` and remove the udev rule
  from `/etc/udev/rules.d`, where the installer puts it.

## [0.1.0] - 2026-09-14

### Added

- `omalogi info`, `omalogi profiles` and `omalogi backup`: device, firmware, DPI and
  report rate; onboard profiles with DPI stages and button bindings; a full backup of
  profile memory. Every command supports `--json`.
- `omalogi profiles activate <N>` switches the active onboard profile and reads it back.
- `omalogi profiles edit <N>` changes DPI stages, the default and DPI-shift stages, the
  report rate and button and G-Shift bindings, with `--dry-run`. Each write is preceded
  by an automatic backup, verified by reading it back, and rolled back on a mismatch.
  `--name` names the profile.
- `omalogi profiles enable <N>` / `disable <N>` turn profiles on and off, with
  `--dry-run`. The profile in use and the last profile turned on are refused.
- `scripts/hardware-selftest.py`: a manual self-test that drives every overlay edit on a
  dedicated test profile against a real mouse and checks the memory is byte for byte
  as before.
- Omalogi's pixel mark, a wired mouse on a 16 × 16 grid, in the overlay header (its
  wheel rolls while a change is written), the bar indicator (it rolls when the profile
  changes, and its wheel is lit while a rule chose the profile) and the README.
- The G-Shift layer names the button that reaches it ("hold G5"), and says so when no
  button holds G-Shift on the profile. Disabled buttons read quieter than the rest.
- `omalogi restore <FILE>` writes profile memory back from a backup, with `--dry-run`.
- `omalogi daemon` switches profiles per app and per monitor from rules in
  `~/.config/omalogi/config.toml`, and publishes its state for the shell plugin.
- Omarchy shell plugin: a G HUB-style editor. The mouse is shown one view at a time, chosen
  with Front and Side picture tiles, with a label and line per button and a Default / G-Shift switch; actions are
  dragged from a grouped, searchable library onto buttons, or picked for the selected
  one, which also offers Use default (the mouse's factory binding), Disable and a
  shortcut recorder. Sensitivity puts the DPI levels on one draggable bar. Changes save
  themselves (about 0.3 s to take effect on the profile in use) with Undo; the session's
  first write is preceded by a backup of all profile memory. Opens on a given profile,
  view or button. Plus a bar indicator for the active profile.
- `omalogi serve`, the overlay's long-lived connection to the mouse: JSON requests on
  stdin, answers on stdout, with an undo stack.
- `omalogi dpi [VALUE]` shows the sensor's live DPI (setting it is refused by the G502 X
  while it runs onboard profiles).
- Every Omalogi process holds a device lock while talking to the mouse, and profile data
  that fails its checksum is read again and otherwise refused.
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
