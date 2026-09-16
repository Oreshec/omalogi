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

## 2026-09-13 — mouse picture and button positions

`omalogi picture` downloaded `metadata.json`, `front.png` and `side.png` for depot `g502x`
(matched on product id c099) from assets.openlogi.org in 1.4 s; each file matched the
size and SHA-256 in the host's index. A second run with `--offline` used the cache only.

The metadata names buttons `g502x_g<N>_m1`. With markers drawn on the renders, each
position was compared with the firmware's default binding for slot N−1 on this mouse:

| Id | Position on the render | Slot N−1 default |
|---|---|---|
| g1, g2, g3 | left button, right button, wheel | left, right, middle click |
| g4 | rear thumb button | back |
| g5 | front thumb button | DPI shift |
| g6 | middle thumb button | forward |
| g7, g8 | wheel tilt left, right | scroll left, scroll right |
| g9 | button below the wheel | cycle profile |
| g10, g11 | upper and lower left-edge buttons | DPI up, DPI down |

All 11 agree, so the overlay maps g*N* to slot N−1 for the G502 X only. `scroll1` and
`scroll2` mark wheel up and down, which have no slot. Result: **consistent**; a physical
press per button has not been done.

## 2026-09-13 — when written profile memory reaches the mouse

Reported problem: DPI edits made in the overlay could not be felt. The two writes had
gone to profile 2 while profile 1 was active. Tested on profile 2, watching the live
sensor DPI (`omalogi info`):

| Step | Live DPI |
|---|---|
| Profile 2 active (default stage 1600) | 1600 |
| Write default stage 2400 to profile 2 | 1600 |
| One second later | 1600 |
| `setCurrentProfile` to profile 2 again (already active) | 1600 |
| Switch to profile 1, then back to profile 2 | **2400** |

Result: the firmware loads a profile's settings only when it switches to that profile.
A write, or selecting the profile that is already active, is not enough. Profile 2 was
restored from the write's backup afterwards.

Fix: after a verified write or restore that changes the active profile, Omalogi switches
to another enabled profile and straight back (under the device lock) and reports
`takes_effect`. Verified with the release build:

| Case | Reported | Live DPI |
|---|---|---|
| Edit profile 2 while profile 1 is active | `when_activated` | 1600, unchanged |
| Edit profile 2 while it is active | `now` | 2400 immediately, profile 2 still active |
| Restore profile 2 from that backup | `now` | 1600 |

Result: **pass**. Profile 1 was active again at the end, and profile 2 was byte-identical
to its state before the tests.

## 2026-09-13 — responsiveness, live DPI and concurrent access

**Live DPI.** `setSensorDpi` (0x2201 function 3) in onboard mode returned a HID++ feature
error and the live DPI stayed unchanged, so a DPI preview without writing a profile is
not possible while the mouse runs onboard profiles.

**Latency.** Measured on the release build, profile 2, everything undone afterwards:

| Step | Before | After |
|---|---|---|
| Overlay save of the profile in use | ~1.26 s (preview, edit, full reread) | 316–325 ms via `omalogi serve` |
| First save of a session (includes the full backup) | — | 693 ms |
| Undo on the profile in use | — | 414 ms |
| Activate a profile | ~0.3 s process start | 76 ms |

Gains came from one long-lived session, building the result from the verified bytes
instead of reading the profile again, and switching profiles during a reload without
reading the directory each time.

**Concurrent access.** Two Omalogi processes talking to the mouse at the same moment are
not safe:

| Test | Result |
|---|---|
| 10 `omalogi serve` starts, each racing a CLI read | 6 took 5.5 s (a request timed out) |
| 10 starts alone, daemon polling | all ~325 ms |
| A `state` read overlapping other processes' memory reads | the directory came back with an invalid checksum |

Fix: every process holds the device lock for all of its traffic (CLI per command, the
server per request, the daemon per poll, as before), and profile data that fails its
checksum is read once more and otherwise refused, never edited or written back.

Stress test after the fix: a loop reading all profiles continuously, 10 server starts
racing CLI reads, and the full save and undo sequence at the same time.

| Check | Result |
|---|---|
| Loop reads | 40 of 40 ok, every checksum valid |
| Server starts | 10 of 10 ok (~0.92 s, waiting for the lock; no timeouts) |
| Saves and undos under load | all ok; profile 2 byte-identical afterwards |

Result: **pass**.

## Full self-test through the overlay's server

2026-09-14, G502 X (wired), firmware U1 60.00.B0009. Profile 3 was turned on with
`omalogi profiles enable 3` and named with `profiles edit 3 --name "Omalogi Test"`
(both verified by reading back), then `scripts/hardware-selftest.py` drove
`omalogi serve` exactly as the overlay does:

| Area | What was checked |
|---|---|
| Bindings | 43 actions (the catalog, 16 key combinations, `button:6`, `button:16`) rotated through all 11 buttons on both layers: every action on every slot, each read back with a label; a fresh read every 10 writes |
| Refusals | 20 bad edits (unknown actions and keys, `button:0`/`17`, slot 16, DPI 50/30000/123, six stages, a default that is not a stage, 333 Hz, long or non-ASCII names, profile 9) wrote nothing |
| Sensitivity | 1 to 5 stages from 100 to 25600 DPI, each stage as default with another as shift, all four report rates |
| Names | set, 47 characters, cleared |
| Undo | three writes undone one by one, each matching the state before it |
| In use | activation; a new default DPI and report rate live at once (`takes_effect: now`); undo live at once; the profile in use and a profile already off refused; profile 4 turned on and off |
| Restore | all profile memory byte for byte as before the test |

A first run found 10 failures, all in the script (unused DPI stages read back as `null`).
The second run: **1979 checks passed, 0 failed, in 30 s**.

| Request | Count | Median | Max |
|---|---|---|---|
| `apply` | 87 | 269 ms | 686 ms (the first, with the backup) |
| `undo` | 6 | 390 ms | 422 ms |
| `state` | 13 | 375 ms | 386 ms |
| `activate` | 4 | 66 ms | 76 ms |
| `set_enabled` | 5 | 52 ms | 578 ms |

Noted, not failures: slot 11 is refused as "not a button on this mouse", and unsorted
stages such as 1600,800 are kept in that order (the overlay always sorts them).

Result: **pass**.

## Observations

- 2026-09-16: during the in-use phase of the self-test, one `omalogi dpi` read right after
  a profile reload failed with `ETIMEDOUT` (os error 110) on the G502 X. The run stopped,
  its automatic restore put all profile memory back byte for byte (confirmed with a fresh
  backup), and the daemon logged nothing. The self-test now retries live-DPI reads twice,
  0.5 s apart, and picks its DPI cases from each sensor's own supported values; the next
  full run passed.

- 2026-09-13 20:00:34: one daemon poll failed with `ETIMEDOUT` (os error 110) from the
  hidraw write, with no other Omalogi traffic; the daemon reconnected at once. USB
  autosuspend is off for the mouse (`power/control` = `on`, never suspended). Cause
  unknown. Follow-up: a cross-process device lock around memory writes, and the daemon
  tolerates a single transient timeout.

## Not yet tested on hardware

- The rollback path after a failed verification (tested on the emulated device only).
- Unplugging the mouse during a write.
- A write started from the overlay editor (preview and rendering are tested).
- Device access through the udev rule alone, after a replug.
