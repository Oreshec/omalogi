# Hardware test log

Manual tests run on real hardware, in order. Each entry lists the device, what was
done, how it was checked independently of Omalogi, and the result. The raw device
dumps referenced here stay local (they contain the device Unit ID); the redacted
fixture in `tests/fixtures/g502x-c099.json` comes from the same dump.

Device for all entries: Logitech G502 X, wired, USB 046d:c099, firmware U1 60.00.B0009,
bootloader BL1 59.00.B0002, HID++ 4.2. Host: Arch Linux (Omarchy 4.0.3, Hyprland 0.56.2).

The independent checker is `research/tools/probe_readonly.py`, a separate Python
implementation that only sends HID++ getters and `memoryRead`.

## 2026-09-13 — read path

| Check | Result |
|---|---|
| `omalogi info` firmware, DPI range, report rates | U1 60.00.B0009; 100–25600 step 50; 125/250/500/1000 Hz; matches Solaar's c099 dump |
| `omalogi backup` vs probe dump | all 9 sectors byte-identical |
| Active profile index | 1-based; confirmed by holding DPI shift (1600 → 800 DPI only on the DPI-shift profile) |
| Coexistence with OpenLogi 0.8.3 agent | 30 Omalogi commands while the agent held the device: 0 failures, backups identical |

## 2026-09-13 — profile switching (RAM only)

| Step | Result |
|---|---|
| `omalogi profiles activate 1` | probe reports profile 1 |
| `omalogi profiles activate 2` (restore) | probe reports profile 2 |
| Activate disabled profile 3 / missing profile 9 | refused with a message, nothing sent |
| Daemon: config `default_profile` 2 → 1 → 2 | switched within one poll; probe confirmed each; broken config reported, last good rules kept; clean SIGTERM exit, state file removed |

## 2026-09-13 — first onboard memory write, on a disabled profile

Procedure (profile 3 is disabled on this mouse, so it is never active):

1. Baseline: `omalogi backup`; all sectors equal the original probe dump.
2. `omalogi profiles edit 3 --rate 500 --dry-run` shows `Report rate 1000 Hz → 500 Hz`.
3. `omalogi profiles edit 3 --rate 500`: exit 0, automatic backup saved first.
4. Probe reads sector `0003` directly: report-rate byte `2` (500 Hz), CRC valid.
5. Fresh backup: only sector `0003` differs from the original, only at bytes 0, 253, 254
   (the report rate and the CRC).
6. `omalogi restore <automatic backup>`: "Restored and verified sectors 0003", with a
   backup of the pre-restore state saved first.
7. Final backup: all 9 sectors byte-identical to the original probe dump; probe reads
   sector `0003` equal to the original; active profile still 2; daemon service active.

Result: **pass**. Writes, read-back verification, automatic backups and restore work on
real hardware, and an edit changes exactly the intended bytes.

## 2026-09-13 — memory write to an enabled profile, applied by switching

Procedure (profile 1 is enabled but not active; profile 2 is active):

1. Baseline backup equals the original probe dump; live DPI 1600.
2. `omalogi profiles edit 1 --dpi 400,800,1600,3200 --default-dpi 400 --dry-run` shows
   `800 1200 [1600] 2400 3200 shift 800 → [400] 800 1600 3200 shift 800` (shift keeps
   its 800 DPI value at its new position).
3. Real write: exit 0, automatic backup saved first.
4. Probe reads sector `0001`: stages 400/800/1600/3200/unused, default index 0, shift
   index 1, CRC valid. Only sector `0001` changed (bytes 1–6, 9–12, CRC).
5. `omalogi profiles activate 1`: live DPI (probe, `getSensorDpi`) reads **400** — the
   device applies a stored profile's settings when it is selected.
6. `omalogi profiles activate 2`: live DPI back to 1600.
7. `omalogi restore <automatic backup>`: sector `0001` restored and verified; all 9
   sectors byte-identical to the original dump; profile 2 active at 1600 DPI.

Result: **pass**.

## 2026-09-13 — device lock between the CLI and the daemon

With the daemon running as a user service, a separate process held
`$XDG_RUNTIME_DIR/omalogi/device.lock` (as `profiles edit` and `restore` do) and the
active profile was switched to 1 underneath it:

| Step | Daemon state |
|---|---|
| Lock held, 7 seconds (two poll intervals) | stayed at profile 2 every second: polls skipped |
| Lock released | profile 1 within one poll |
| `omalogi profiles activate 2` | profile 2 |

Result: **pass**. The daemon does not touch the device while a memory write holds the lock.

## 2026-09-13 — udev rule

`packaging/udev/70-omalogi.rules` installed to `/usr/lib/udev/rules.d/`, rules reloaded,
hidraw change event triggered, mouse not replugged:

| Node | USB interface | Tags |
|---|---|---|
| hidraw7 | 00 (mouse input) | `:seat:` |
| hidraw8 | 01 (HID++) | `:seat:uaccess:` |

`omalogi info` opened hidraw8. Result: **pass** for matching: only the HID++ interface is
tagged. The session ACL on hidraw8 was already present from an earlier manual `setfacl`,
so access coming from the rule alone is confirmed after the next replug.

## Observations

- 2026-09-13 20:00:34: one daemon poll failed with `ETIMEDOUT` (os error 110) from the
  hidraw write, with no other Omalogi traffic; the daemon reconnected at once. USB
  autosuspend is off for the mouse (`power/control` = `on`, never suspended). Cause
  unknown. Follow-up: a cross-process device lock around memory writes, and the daemon
  tolerates a single transient timeout.

## Not yet tested on hardware

- Button binding writes, and whether edits to the currently active profile apply
  without re-selecting it.
- The rollback path after a failed verification (tested on the emulated device only).
- Unplugging the mouse during a write.
- A write started from the overlay editor (preview and rendering are tested).
- Device access through the udev rule alone, after a replug.
