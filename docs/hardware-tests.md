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

## Not yet tested on hardware

- Writes to an enabled or active profile (DPI stages, bindings), and whether a change
  to the active profile applies immediately or after re-selecting it.
- The rollback path after a failed verification (tested on the emulated device only).
- Unplugging the mouse during a write.
