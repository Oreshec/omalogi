# Omalogi for coding agents

This file is for any coding agent (Codex, Claude Code, Cursor, Copilot, Gemini CLI,
opencode, …) working in this repository. Humans should read
[CONTRIBUTING.md](../../CONTRIBUTING.md); the rules are the same.

It lives here rather than in the repository root, and is named so no agent loads it by
itself: `omarchy plugin add` clones this repository into the user's plugins folder, and a
published plugin must carry no instructions an agent could pick up on its own. Point your
agent at this file yourself.

Omalogi configures Logitech G-series mice on [Omarchy](https://omarchy.org). It is built
in Omarchy, for Omarchy: an omarchy-shell plugin (QML) in front of a Rust helper that
speaks HID++ 2.0 to the mouse. Ready-made task prompts are in
[docs/agents/](README.md).

## Map

| Path | What |
|---|---|
| `manifest.json`, `plugin/` | The Omarchy plugin: overlay (`Omalogi.qml`), bar widget (`Indicator.qml`), pure JS model (`Model.js`) |
| `plugin/tests/` | Node tests for `Model.js`; UI logic belongs in `Model.js` so it is testable |
| `src/hidraw.rs`, `src/device.rs` | Device discovery, transport, sessions |
| `src/onboard/` | Onboard profile memory: format, decoding, editing, actions |
| `src/editing.rs` | Validated edits, backups, verified writes, rollback, restore |
| `src/serve.rs` | `omalogi serve`, the JSON-lines connection the overlay uses |
| `src/setup.rs` | `omalogi setup`; `PLUGIN_FILES` must list every file in `plugin/` |
| `src/daemon.rs`, `src/rules.rs`, `src/hyprland.rs` | Per-app and per-monitor switching |
| `tests/` | End-to-end tests against an emulated G502 X (`tests/support`) |
| `scripts/hardware-selftest.py` | The manual hardware matrix, run on a dedicated test profile |
| `docs/hardware-tests.md` | Everything verified on real hardware, and how |

## Checks

Run all of these before you say a change is done. CI runs the same, and `main` requires them.

```sh
cargo fmt
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test "plugin/tests/**/*.test.js"
/usr/lib/qt6/bin/qmllint plugin/<changed>.qml   # errors only; Omarchy types show as unresolved
```

No test needs a mouse. If you change behaviour, add or update a test in the same change.

## Hard rules

These protect people's hardware and privacy. Do not work around them, and stop and ask
the human when a task seems to need it.

1. **Never flash firmware and never write factory (ROM) sectors.** There is no code for
   it; do not add any.
2. **No guessing about devices.** Every protocol detail cites a source (libratbag,
   Solaar, OpenLogi, Logitech documents) or a measurement on a real device, written in
   the code comment or `docs/hardware-tests.md`.
3. **Writes are proven on the emulator first**, then on hardware with a dry run, a
   backup and a restore. Only a human with the mouse can run hardware steps: ask them,
   give them the exact commands, and log the results in `docs/hardware-tests.md`.
4. **Readable is not verified.** `DECODABLE_LAYOUTS` are the layouts Omalogi can edit;
   a mouse counts as verified only with `verified` in `SUPPORTED_DEVICES` and its layout
   in `VERIFIED_LAYOUTS`. Never mark either verified without a byte-by-byte check and the
   hardware tests on a real device, and never bypass the untested-mouse acceptance.
5. **Never commit device identity.** Raw dumps and backups contain the mouse's unit ID.
   Fixtures in `tests/fixtures/` have it zeroed in `device_info`; never commit a raw
   dump, a backup file, or a screenshot showing anything but Omalogi.
6. **Never commit secrets, personal paths, or your own session links, prompts or notes.**
   Keep scratch files out of the repo.
7. **One device process at a time.** Every process that talks to the mouse holds the
   device lock (`src/lock.rs`). Do not add traffic outside it.

## Working on the overlay

- Omarchy components come from `qs.Commons` and `qs.Ui`; follow the existing files and the
  user's theme (`Color.*`, `Style.*`). Never hardcode colours or pixel sizes.
- Put logic in `Model.js` with a Node test; keep QML declarative.
- A running shell does not reload a changed overlay. To try a change:
  `cargo build --release && target/release/omalogi setup --no-bar --no-daemon`, then
  `omarchy restart shell`. A new QML file must also be added to `PLUGIN_FILES` in
  `src/setup.rs`.
- The overlay takes keyboard focus. On someone's desktop, open it only when they agree,
  close it when done (`omarchy-shell shell hide io.github.elberacasa.omalogi`), and crop
  screenshots to the overlay.
- Shell errors: `quickshell log -p "$OMARCHY_PATH/shell" --any-display | grep -i omalogi`.

## Style

- Match the surrounding code: naming, comment density, error messages that say what went
  wrong and how to fix it.
- User-facing text names what people recognise ("profile", "button", "DPI level"), in
  plain, active sentences.
- Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`), one logical change per
  commit, through a pull request.
