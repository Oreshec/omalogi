#!/usr/bin/env python3
"""Read-only HID++ 2.0 probe for the wired Logitech G502 X (046d:c099).

Phase 0 research tooling, not project code. Safety properties:
  * The node is found through sysfs: vendor 046d, product c099, and a report
    descriptor containing HID++ short/long reports (vendor page 0xFF00, IDs
    0x10/0x11). Nothing is hardcoded to a hidraw number.
  * Only (feature, function) pairs in READ_ONLY_CALLS can be sent. Each is a
    getter, with function numbers cited from libratbag src/hidpp20.c and Solaar
    lib/logitech_receiver/hidpp20.py. Setters and flash writes (0x8100
    functions 1, 3, 6, 7, 8) are not in the table and raise before any write.
  * Memory reads are capped at MAX_SECTORS and stay inside the sector size
    the device reports.
  * Every raw request/response and every sector read is saved to a JSON
    fixture. That file is also the onboard-profile backup taken before any
    future write. Decoding here is best-effort; the raw bytes are the record.
"""

from __future__ import annotations

import json
import os
import select
import sys
import time
from pathlib import Path

VENDOR, PRODUCT = 0x046D, 0xC099
REPORT_SHORT, REPORT_LONG = 0x10, 0x11
LEN = {REPORT_SHORT: 7, REPORT_LONG: 20}
DEVICE_INDEX = 0xFF  # corded device addressed directly (no receiver)
SW_ID = 0x0A
MAX_SECTORS = 16

FEATURE_ROOT = 0x0000
FEATURE_SET = 0x0001
FEATURE_FW_INFO = 0x0003
FEATURE_NAME = 0x0005
FEATURE_DPI = 0x2201
FEATURE_REPORT_RATE = 0x8060
FEATURE_ONBOARD = 0x8100

# (feature id, function) -> spec name. Getter functions only.
READ_ONLY_CALLS = {
    (FEATURE_ROOT, 0): "Root.getFeature",
    (FEATURE_ROOT, 1): "Root.getProtocolVersion",
    (FEATURE_SET, 0): "FeatureSet.getCount",
    (FEATURE_SET, 1): "FeatureSet.getFeatureID",
    (FEATURE_FW_INFO, 0): "DeviceFwInfo.getDeviceInfo",
    (FEATURE_FW_INFO, 1): "DeviceFwInfo.getFwInfo",
    (FEATURE_NAME, 0): "DeviceName.getCount",
    (FEATURE_NAME, 1): "DeviceName.getDeviceName",
    (FEATURE_DPI, 0): "AdjustableDPI.getSensorCount",
    (FEATURE_DPI, 1): "AdjustableDPI.getSensorDpiList",
    (FEATURE_DPI, 2): "AdjustableDPI.getSensorDpi",
    (FEATURE_REPORT_RATE, 0): "ReportRate.getReportRateList",
    (FEATURE_REPORT_RATE, 1): "ReportRate.getReportRate",
    (FEATURE_ONBOARD, 0): "OnboardProfiles.getDescription",
    (FEATURE_ONBOARD, 2): "OnboardProfiles.getMode",
    (FEATURE_ONBOARD, 4): "OnboardProfiles.getCurrentProfile",
    (FEATURE_ONBOARD, 5): "OnboardProfiles.memoryRead",
}


def find_hidpp_node() -> Path:
    for node in sorted(Path("/sys/class/hidraw").iterdir()):
        uevent = (node / "device/uevent").read_text()
        if f"HID_ID=0003:{VENDOR:08X}:{PRODUCT:08X}" not in uevent:
            continue
        desc = (node / "device/report_descriptor").read_bytes()
        # 06 00 ff = Usage Page (Vendor 0xFF00); 85 10 / 85 11 = Report IDs.
        if b"\x06\x00\xff" in desc and b"\x85\x10" in desc and b"\x85\x11" in desc:
            return Path("/dev") / node.name
    sys.exit("G502 X HID++ interface not found (is the mouse plugged in?)")


def crc_ccitt(data: bytes) -> int:
    """CRC-16-CCITT (init 0xFFFF, poly 0x1021), as libratbag hidpp-generic.c."""
    crc = 0xFFFF
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else (crc << 1)
            crc &= 0xFFFF
    return crc


class Probe:
    def __init__(self, dev: Path) -> None:
        self.dev = dev
        try:
            self.fd = os.open(dev, os.O_RDWR | os.O_NONBLOCK)
        except PermissionError:
            sys.exit(
                f"Permission denied on {dev}. Grant temporary access with:\n"
                f"  sudo setfacl -m u:{os.getlogin()}:rw {dev}\n"
                "(resets when the mouse is replugged or on reboot)"
            )
        self.index = {FEATURE_ROOT: 0x00}
        self.log: list[dict] = []

    def call(self, feature: int, function: int, *params: int, timeout: float = 1.0) -> bytes:
        if (feature, function) not in READ_ONLY_CALLS:
            raise PermissionError(f"refusing non-allowlisted call {feature:#06x}/{function}")
        idx = self.index[feature]
        payload = bytes(params)
        report = REPORT_SHORT if len(payload) <= 3 else REPORT_LONG
        req = bytes([report, DEVICE_INDEX, idx, (function << 4) | SW_ID]) + payload
        req = req.ljust(LEN[report], b"\x00")
        self._drain()
        os.write(self.fd, req)
        deadline = time.monotonic() + timeout
        while (left := deadline - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], left)[0]:
                break
            resp = os.read(self.fd, 64)
            if len(resp) < 5 or resp[0] not in LEN or resp[1] != DEVICE_INDEX:
                continue
            is_error = resp[2] == 0xFF and resp[3] == idx and resp[4] == req[3]
            if not is_error and (resp[2] != idx or resp[3] != req[3]):
                continue  # unrelated event or reply
            self.log.append({
                "call": READ_ONLY_CALLS[(feature, function)],
                "request": req.hex(),
                "response": resp.hex(),
                "error": is_error,
            })
            if is_error:
                raise RuntimeError(
                    f"{READ_ONLY_CALLS[(feature, function)]} -> HID++ error {resp[5]:#04x}"
                )
            return resp[4:]
        raise TimeoutError(f"no reply to {READ_ONLY_CALLS[(feature, function)]}")

    def read_sector(self, sector: int, size: int) -> bytes:
        """Read a whole sector 16 bytes at a time, never past `size`."""
        data = bytearray(size)
        offset = 0
        while offset < size:
            # A read must not cross the sector end; the last one realigns to size-16.
            at = min(offset, size - 16)
            chunk = self.call(FEATURE_ONBOARD, 5, sector >> 8, sector & 0xFF, at >> 8, at & 0xFF)
            data[at:at + 16] = chunk[:16]
            offset = at + 16
        return bytes(data)

    def _drain(self) -> None:
        while select.select([self.fd], [], [], 0)[0]:
            os.read(self.fd, 64)


def u16(b: bytes, i: int) -> int:
    return (b[i] << 8) | b[i + 1]


def probe_onboard(p: Probe) -> dict:
    desc = p.call(FEATURE_ONBOARD, 0)
    info = {
        "description_raw": desc.hex(),
        "decoded_best_effort": {
            "memory_model": desc[0],
            "profile_format": desc[1],
            "macro_format": desc[2],
            "profile_count": desc[3],
            "rom_profile_count": desc[4],
            "button_count": desc[5],
            "sector_count": desc[6],
            "sector_size": u16(desc, 7),
            "mechanical_layout": f"{desc[9]:08b}",
        },
        "mode_raw": p.call(FEATURE_ONBOARD, 2).hex(),
        "current_profile_raw": p.call(FEATURE_ONBOARD, 4).hex(),
    }
    size = info["decoded_best_effort"]["sector_size"]
    if not 16 <= size <= 4096:
        info["sectors_skipped"] = f"implausible sector size {size}"
        return info

    sectors: dict[str, dict] = {}

    def keep(sector: int) -> bytes:
        raw = p.read_sector(sector, size)
        sectors[f"{sector:04x}"] = {
            "raw": raw.hex(),
            "crc_ok": crc_ccitt(raw[:-2]) == u16(raw, size - 2),
        }
        return raw

    # User directory (0x0000), then ROM directory (0x0100) when ROM profiles exist.
    directories = [0x0000] + ([0x0100] if info["decoded_best_effort"]["rom_profile_count"] else [])
    for directory in directories:
        raw = keep(directory)
        for i in range(0, size - 2, 4):
            sector = u16(raw, i)
            if sector == 0xFFFF or len(sectors) >= MAX_SECTORS:
                break
            if f"{sector:04x}" not in sectors:
                keep(sector)
    info["sectors"] = sectors
    return info


def main() -> None:
    dev = find_hidpp_node()
    p = Probe(dev)
    out: dict = {"node": str(dev), "vid_pid": f"{VENDOR:04x}:{PRODUCT:04x}"}

    ver = p.call(FEATURE_ROOT, 1)
    out["protocol"] = f"{ver[0]}.{ver[1]}"

    for fid in (FEATURE_SET, FEATURE_FW_INFO, FEATURE_NAME,
                FEATURE_DPI, FEATURE_REPORT_RATE, FEATURE_ONBOARD):
        r = p.call(FEATURE_ROOT, 0, fid >> 8, fid & 0xFF)
        if r[0]:
            p.index[fid] = r[0]

    count = p.call(FEATURE_SET, 0)[0]
    out["features"] = []
    for i in range(1, count + 1):
        r = p.call(FEATURE_SET, 1, i)
        out["features"].append({
            "index": i,
            "id": f"{u16(r, 0):04x}",
            "type_flags": f"{r[2]:08b}",
            "version": r[3],
        })

    if FEATURE_NAME in p.index:
        n = p.call(FEATURE_NAME, 0)[0]
        name = b""
        while len(name) < n:
            name += p.call(FEATURE_NAME, 1, len(name))[: n - len(name)]
        out["name"] = name.rstrip(b"\x00").decode(errors="replace")

    if FEATURE_FW_INFO in p.index:
        info = p.call(FEATURE_FW_INFO, 0)
        out["fw_device_info_raw"] = info.hex()
        out["fw_entities"] = [p.call(FEATURE_FW_INFO, 1, e).hex() for e in range(info[0])]

    if FEATURE_DPI in p.index:
        sensors = p.call(FEATURE_DPI, 0)[0]
        out["dpi"] = [{
            "sensor": s,
            "dpi_list_raw": p.call(FEATURE_DPI, 1, s).hex(),
            "current_raw": p.call(FEATURE_DPI, 2, s).hex(),
        } for s in range(sensors)]

    if FEATURE_REPORT_RATE in p.index:
        out["report_rate"] = {
            "list_bitmap_raw": p.call(FEATURE_REPORT_RATE, 0).hex(),
            "current_raw": p.call(FEATURE_REPORT_RATE, 1).hex(),
        }

    if FEATURE_ONBOARD in p.index:
        out["onboard_profiles"] = probe_onboard(p)

    out["exchanges"] = p.log
    dest = Path(__file__).with_name("g502x-c099-probe.json")
    dest.write_text(json.dumps(out, indent=2) + "\n")
    summary = {k: v for k, v in out.items() if k not in ("exchanges",)}
    if "onboard_profiles" in summary:
        onboard = dict(summary["onboard_profiles"])
        onboard["sectors"] = {k: {"crc_ok": v["crc_ok"]} for k, v in onboard.get("sectors", {}).items()}
        summary["onboard_profiles"] = onboard
    print(json.dumps(summary, indent=2))
    print(f"\n{len(p.log)} exchanges saved to {dest}", file=sys.stderr)


if __name__ == "__main__":
    main()
