# Contributing

Thanks for helping. Omalogi writes to people's hardware, so the bar is correctness first:
nothing about a device is assumed, and every claim about a device is verified on one.

Working with a coding agent? Point it at [docs/agents/agent-guide.md](docs/agents/agent-guide.md)
and the task prompts beside it; the rules there are these rules.

## Development loop

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
node --test plugin/tests
```

CI runs the same checks. Tests never need a mouse: `tests/support` emulates a wired
G502 X from a real, redacted device dump, including memory writes and a mode that
corrupts a write to exercise rollback.

Trying the plugin on your desktop:

```sh
cargo build --release
D=~/.config/omarchy/plugins/io.github.elberacasa.omalogi
mkdir -p "$D" && cp -r manifest.json plugin "$D"/
omarchy-shell shell rescanPlugins
```

Copy the files rather than symlinking them; Omarchy's plugin validator rejects symlinks.
After adding a new QML file the shell must be restarted once, because Qt caches the
plugin directory listing. After rebuilding, restart the daemon service so it runs the
new binary: `systemctl --user restart omalogi`.

## Layout

| Path | What |
|---|---|
| `src/hidraw.rs` | hidraw discovery and transport |
| `src/device.rs` | a session with a device: reads, profile switching, backups |
| `src/onboard/` | onboard profiles: memory format, decoding, labels, editing, actions |
| `src/editing.rs` | validated edits, verified writes, restore |
| `src/rules.rs`, `src/hyprland.rs`, `src/daemon.rs` | automatic switching |
| `src/lock.rs` | keeps processes off the device during writes |
| `plugin/` | omarchy-shell overlay, bar indicator and their pure JS model |
| `tests/` | end-to-end tests against the emulated device |
| `research/tools/probe_readonly.py` | independent read-only HID++ probe |
| `docs/hardware-tests.md` | what was verified on real hardware, and how |

## Rules for device code

- **No guessing.** Protocol details come from a cited source (libratbag, Solaar,
  OpenLogi, Logitech documents) or are measured on a real device. Write down which.
- **Read before write.** New write paths are proven against the emulated device first,
  then on hardware with a dry run, a backup and a restore, and logged in
  `docs/hardware-tests.md`.
- **Readable is not verified.** Omalogi reads and edits the layouts in
  `DECODABLE_LAYOUTS` (`src/onboard/format.rs`), which libratbag lays out identically.
  A mouse is verified only when its model has `verified` in `SUPPORTED_DEVICES`
  (`src/hidraw.rs`) and its layout is in `VERIFIED_LAYOUTS`; every other mouse needs the
  user's one-time acceptance before a write. Mark a model verified only after its
  profile sectors are checked byte by byte and the hardware tests pass on a real one.
- **Never** write factory sectors or firmware.

## Adding a device

1. Dump it read-only with `research/tools/probe_readonly.py` (grant access to its hidraw
   node for the session with `sudo setfacl -m u:$USER:rw /dev/hidrawN`). The probe only
   sends getters and memory reads.
2. Compare the feature list and onboard description with Solaar's or libratbag's data
   for the device; note any differences.
3. Decode each user profile sector and check DPI stages and bindings against what the
   mouse actually does (for example, hold DPI shift and read the live DPI).
4. Create a redacted fixture in `tests/fixtures/`: zero the Unit ID bytes in
   `device_info` and never commit the raw dump.
5. Add the device to `SUPPORTED_DEVICES` in `src/hidraw.rs`, its layout to
   `VERIFIED_LAYOUTS` if new, and a line to `packaging/udev/70-omalogi.rules`.
6. Run and log the hardware tests, then list the device as tested in the README.

## Commits

One logical change per commit, [Conventional Commits](https://www.conventionalcommits.org)
messages (`feat:`, `fix:`, `docs:`, `chore:`), and CI green before merging.
