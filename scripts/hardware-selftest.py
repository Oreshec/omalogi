#!/usr/bin/env python3
"""Hardware self-test: every edit the overlay makes, checked against the mouse itself.

Drives `omalogi serve` exactly as the overlay does, on a dedicated test profile that is
turned on, not in use, and named "Omalogi Test":

  omalogi profiles enable 3 && omalogi profiles edit 3 --name "Omalogi Test"
  scripts/hardware-selftest.py --profile 3

It writes the mouse's onboard memory, so it is never run by CI. Button bindings only
change while the test profile is not in use; while it is, only the DPI, the report rate
and one harmless button change, so the mouse stays usable throughout. All profile memory
is backed up first and compared byte for byte at the end, and restored if anything
differs.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import tempfile
import time

KEYS = [
    "key:a", "key:z", "key:1", "key:enter", "key:esc", "key:tab", "key:space",
    "key:minus", "key:f13", "key:f24", "key:up", "key:ctrl+t", "key:ctrl+shift+t",
    "key:alt+tab", "key:super+enter", "key:ctrl+shift+alt+super+f13",
]
BUTTONS = ["button:6", "button:16"]


class Serve:
    def __init__(self, binary):
        self.process = subprocess.Popen(
            [binary, "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
        )
        self.last_id = 0
        self.times = {}

    def request(self, cmd, **fields):
        self.last_id += 1
        started = time.monotonic()
        self.process.stdin.write(json.dumps({"id": self.last_id, "cmd": cmd, **fields}) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError("omalogi serve exited")
        self.times.setdefault(cmd, []).append((time.monotonic() - started) * 1000)
        response = json.loads(line)
        assert response["id"] == self.last_id, response
        return response

    def result(self, cmd, **fields):
        response = self.request(cmd, **fields)
        if not response["ok"]:
            raise RuntimeError(f"{cmd} failed: {response['error']}")
        return response["result"]

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=10)


class Report:
    def __init__(self):
        self.passed = 0
        self.failed = []
        self.notes = []

    def check(self, name, ok, detail=""):
        if ok:
            self.passed += 1
        else:
            self.failed.append(f"{name}: {detail}")
            print(f"  FAIL {name}: {detail}", flush=True)

    def note(self, text):
        self.notes.append(text)
        print(f"  note {text}", flush=True)


def cli(binary, *args):
    run = subprocess.run([binary, "--json", *args], capture_output=True, text=True)
    if run.returncode:
        raise RuntimeError(f"omalogi {' '.join(args)}: {run.stderr.strip()}")
    return json.loads(run.stdout)


def live_dpi(binary, attempts=3):
    """The sensor's live DPI. Right after loading a profile the mouse can miss one request
    (a USB timeout, seen once on a G502 X), so a read is tried again briefly before it
    counts as a failure. Only reads are retried; writes never are."""
    for attempt in range(attempts):
        try:
            return cli(binary, "dpi")["dpi"]
        except RuntimeError:
            if attempt == attempts - 1:
                raise
            time.sleep(0.5)


def rate_hz(profile):
    return round(1000 / profile["report_rate_ms"])


def changes_for(profile, actions, count):
    stages = profile["dpi_stages"]
    changes = {
        "dpi": [dpi for dpi in stages if dpi is not None],
        "default_dpi": stages[profile["default_dpi_index"]],
        "rate": rate_hz(profile),
        "name": profile["name"] or "",
        "buttons": {str(s): ac for s, ac in enumerate(actions["buttons"][:count]) if ac is not None},
        "gshift": {str(s): ac for s, ac in enumerate(actions["gshift_buttons"][:count]) if ac is not None},
    }
    if profile["shift_dpi_index"] is not None and profile["shift_dpi_index"] < len(stages):
        changes["shift_dpi"] = stages[profile["shift_dpi_index"]]
    return changes


def sector_differences(first, second):
    a = json.load(open(first))["sectors"]
    b = json.load(open(second))["sectors"]
    return sorted(key for key in set(a) | set(b) if a.get(key) != b.get(key))


def run(serve, binary, number, report):
    state = serve.result("state")
    onboard = state["onboard"]
    position = number - 1
    slot = onboard["profiles"][position]
    count = onboard["description"]["button_count"]
    home = onboard["active_position"] + 1
    home_profile = onboard["profiles"][home - 1]["profile"]
    snapshot, snapshot_actions = slot["profile"], slot["actions"]
    last_undo = 0

    def written(response, name):
        nonlocal last_undo
        if not response["ok"]:
            report.check(name, False, response["error"])
            return None
        result = response["result"]
        if result["takes_effect"] is not None:
            last_undo = result["undo"]
        return result

    def reread_matches(result, name):
        profile = serve.result("state")["onboard"]["profiles"][position]
        report.check(
            f"{name}: a fresh read matches the write",
            profile["profile"] == result["slot"]["profile"]
            and profile["actions"] == result["slot"]["actions"],
            "the mouse holds something else",
        )

    # 1. Every action on every slot of both layers. Each write rotates the whole list one
    # step, so N writes put each of the N actions on each slot once.
    print(f"[1/6] bindings: every action on each of {count} slots, both layers", flush=True)
    catalog = [action["value"] for action in cli(binary, "actions") if action["value"] != "key:"]
    actions = catalog + KEYS + BUTTONS
    # Slots whose snapshot binding the text catalog cannot spell come back byte for byte
    # only if nothing overwrites them: sending their action back as text normalizes the
    # trailing reserved and profile bytes to 0. The G502 Hero's factory slots, for
    # example, store known special actions with the tail 0xffff; the G502 X stores them
    # with 0x0000 and no slot is skipped. Leaving these slots alone is exactly what the
    # overlay does, so the test exercises the typable slots and keeps the rest verbatim.
    untypeable = {
        s for s in range(count) if snapshot_actions["buttons"][s] is None
        or snapshot_actions["gshift_buttons"][s] is None
    }
    if untypeable:
        report.note(f"slots {sorted(untypeable)} are not typeable from text and stay verbatim")
    for step in range(len(actions)):
        tables = {
            "buttons": {
                str(s): actions[(step + s) % len(actions)]
                for s in range(count) if s not in untypeable
            },
            "gshift_buttons": {
                str(s): actions[(step + s + count) % len(actions)]
                for s in range(count) if s not in untypeable
            },
        }
        result = written(
            serve.request(
                "apply", profile=number, buttons=tables["buttons"], gshift=tables["gshift_buttons"]
            ),
            f"bindings write {step}",
        )
        if result is None:
            continue
        state = (result["takes_effect"] or {}).get("state")
        report.check(f"bindings write {step} waits for activation", state == "when_activated", state)
        for layer, table in tables.items():
            for s, want in table.items():
                have = result["slot"]["actions"][layer][int(s)]
                report.check(f"{layer}[{s}] = {want}", have == want, f"read back {have}")
                if want != "disabled":
                    label = result["slot"]["labels"][layer][int(s)]
                    report.check(f"{layer}[{s}] {want} has a label", bool(label), "no label")
        if step % 10 == 9:
            reread_matches(result, f"bindings write {step}")

    probe = serve.request("apply", profile=number, buttons={str(count): "key:a"})
    if probe["ok"]:
        report.note(f"slot {count} (past the {count} buttons) accepts a binding")
        serve.result("undo")
    else:
        report.note(f"slot {count} (past the {count} buttons): {probe['error']}")

    # 2. Refusals write nothing: the next unchanged apply still reports the same undo depth.
    print("[2/6] refusals", flush=True)
    refusals = {
        "an unknown action": {"buttons": {"0": "bogus"}},
        "an empty key": {"buttons": {"0": "key:"}},
        "an unknown key": {"buttons": {"0": "key:nosuchkey"}},
        "button:0": {"buttons": {"0": "button:0"}},
        "button:17": {"buttons": {"0": "button:17"}},
        "an unknown media key": {"gshift": {"0": "media:nope"}},
        "slot 16": {"buttons": {"16": "left"}},
        "a slot that is not a number": {"buttons": {"x": "left"}},
        "no DPI stages": {"dpi": []},
        "50 DPI": {"dpi": [50], "default_dpi": 50},
        "30000 DPI": {"dpi": [30000], "default_dpi": 30000},
        "an unsupported DPI step": {"dpi": [123], "default_dpi": 123},
        "six DPI stages": {"dpi": [400, 800, 1600, 2400, 3200, 6400]},
        "a default DPI that is not a stage": {"default_dpi": 999},
        "333 Hz": {"rate": 333},
        "a 48 character name": {"name": "x" * 48},
        "a name that is not ASCII": {"name": "Ömalogi"},
    }
    for name, changes in refusals.items():
        response = serve.request("apply", profile=number, **changes)
        report.check(f"refuses {name}", not response["ok"], "was accepted")
        if response["ok"] and response["result"]["takes_effect"] is not None:
            serve.result("undo")
    for cmd, fields, name in [
        ("activate", {"profile": 9}, "activating profile 9"),
        ("set_enabled", {"profile": 9, "enabled": True}, "turning on profile 9"),
        ("apply", {"profile": 9, "rate": 1000}, "editing profile 9"),
    ]:
        report.check(f"refuses {name}", not serve.request(cmd, **fields)["ok"], "was accepted")

    current = serve.result("state")["onboard"]["profiles"][position]["actions"]["buttons"][0]
    unchanged = serve.result("apply", profile=number, buttons={"0": current})
    report.check("an unchanged apply writes nothing", unchanged["takes_effect"] is None, unchanged)
    report.check(
        "refusals left the undo depth alone", unchanged["undo"] == last_undo,
        f"{unchanged['undo']} after {last_undo}",
    )
    normalized = written(
        serve.request("apply", profile=number, buttons={"0": "key:shift+ctrl+t"}),
        "modifier order",
    )
    if normalized:
        have = normalized["slot"]["actions"]["buttons"][0]
        report.check("modifiers read back in canonical order", have == "key:ctrl+shift+t", have)

    # 3. DPI stage lists, default and shift stages, and every report rate.
    print("[3/6] sensitivity: DPI stages, default and shift stages, report rates", flush=True)
    info_rates = state_info_rates(serve)
    lists = dpi_cases(serve.result("state")["info"]["dpi_values"])
    case = 0
    for stages in lists:
        for default in stages if len(stages) <= 3 else [stages[0], stages[-1]]:
            shift = next((dpi for dpi in stages if dpi != default), default)
            rate = info_rates[case % len(info_rates)]
            case += 1
            name = f"{stages} default {default} shift {shift} at {rate} Hz"
            result = written(
                serve.request("apply", profile=number, dpi=stages, default_dpi=default,
                              shift_dpi=shift, rate=rate),
                name,
            )
            if result is None:
                continue
            profile = result["slot"]["profile"]
            # The profile has five stage slots; unused ones read back as null.
            used = [dpi for dpi in profile["dpi_stages"] if dpi is not None]
            got = (used, profile["dpi_stages"][profile["default_dpi_index"]],
                   profile["dpi_stages"][profile["shift_dpi_index"]], rate_hz(profile))
            report.check(name, got == (stages, default, shift, rate), f"read back {got}")
    reread_matches(result, "sensitivity")
    unsorted = serve.request("apply", profile=number, dpi=[1600, 800], default_dpi=800, shift_dpi=1600)
    if unsorted["ok"]:
        report.note(f"unsorted stages read back as {unsorted['result']['slot']['profile']['dpi_stages']}")
    else:
        report.note(f"unsorted stages: {unsorted['error']}")

    # 4. Names.
    print("[4/6] names", flush=True)
    for name in ["Selftest 0123 ~!", "x" * 47, ""]:
        result = written(serve.request("apply", profile=number, name=name), f"name {name!r}")
        if result:
            have = result["slot"]["profile"]["name"]
            report.check(f"name {name!r}", have == (name or None), f"read back {have!r}")

    # 5. Undo walks back through each write, and the snapshot comes back in one write.
    print("[5/6] undo and restore", flush=True)
    before = serve.result("state")["onboard"]["profiles"][position]
    history = [(before["profile"], before["actions"])]
    for changes in [{"dpi": [800, 1600], "default_dpi": 800, "shift_dpi": 1600},
                    {"buttons": {"3": "key:f13"}}, {"gshift": {"2": "media:mute"}}]:
        result = written(serve.request("apply", profile=number, **changes), f"undo setup {changes}")
        if result:
            history.append((result["slot"]["profile"], result["slot"]["actions"]))
    for index in range(len(history) - 2, -1, -1):
        result = serve.result("undo")
        have = (result["slot"]["profile"], result["slot"]["actions"])
        report.check(f"undo back to step {index}", have == history[index], "did not match")

    result = written(
        serve.request("apply", profile=number, **changes_for(snapshot, snapshot_actions, count)),
        "restore the snapshot",
    )
    restored = bool(result) and result["slot"]["profile"] == snapshot \
        and result["slot"]["actions"] == snapshot_actions
    report.check("the snapshot comes back in one write", restored, "did not match")
    if not restored:
        report.note("skipped activation checks: the test profile is not back to its snapshot")
        return home

    # 6. In use: every change is loaded right away, and undo loads the old settings back.
    print("[6/6] in use: live reload, undo, turning profiles on and off", flush=True)
    stages = snapshot["dpi_stages"]
    default = stages[snapshot["default_dpi_index"]]
    serve.result("activate", profile=number)
    report.check("activation", serve.result("state")["onboard"]["active_position"] == position)
    report.check("live DPI after activating", live_dpi(binary) == default)

    # A snapshot with a single DPI stage has no other stage to switch the default to,
    # so that live check only runs when another valid stage exists.
    others = [dpi for dpi in stages if dpi is not None and dpi != default]
    other_rate = 500 if rate_hz(snapshot) != 500 else 1000
    in_use = []
    if others:
        other_dpi = others[0]
        in_use.append(({"default_dpi": other_dpi}, f"default DPI {other_dpi}",
                       lambda other=other_dpi: live_dpi(binary) == other))
    in_use += [
        ({"rate": other_rate}, f"report rate {other_rate} Hz",
         lambda: serve.result("state")["info"]["report_rate_hz"] == other_rate),
        ({"buttons": {"10": "key:f13"}}, "DPI down as F13", lambda: True),
    ]
    if not others:
        report.note("single DPI stage: the default DPI switch live check is skipped")
    undone = 0
    for changes, name, live in in_use:
        result = written(serve.request("apply", profile=number, **changes), name)
        if result:
            state = (result["takes_effect"] or {}).get("state")
            report.check(f"{name} is loaded right away", state == "now", state)
            report.check(f"{name} is live", live(), "the mouse still uses the old setting")
            undone += 1
    for _ in range(undone):
        state = (serve.result("undo")["takes_effect"] or {}).get("state")
        report.check("undo in use is loaded right away", state == "now", state)
    report.check("live DPI after undo", live_dpi(binary) == default)
    report.check("report rate after undo",
                 serve.result("state")["info"]["report_rate_hz"] == rate_hz(snapshot))

    report.check("refuses turning off the profile in use",
                 not serve.request("set_enabled", profile=number, enabled=False)["ok"], "was accepted")
    off = next((p["position"] + 1 for p in onboard["profiles"] if not p["enabled"]), None)
    if off is None:
        report.note("every profile is turned on: the off-profile toggle checks are skipped")
    else:
        report.check("refuses turning off a profile that is off",
                     not serve.request("set_enabled", profile=off, enabled=False)["ok"], "was accepted")
        for enabled in (True, False):
            result = serve.result("set_enabled", profile=off, enabled=enabled)
            report.check(f"profile {off} turned {'on' if enabled else 'off'}",
                         result["onboard"]["profiles"][off - 1]["enabled"] == enabled)

    serve.result("activate", profile=home)
    home_default = home_profile["dpi_stages"][home_profile["default_dpi_index"]]
    report.check("live DPI back on the home profile", live_dpi(binary) == home_default)
    return home



def dpi_cases(values):
    """Stage lists this sensor supports: one to four common levels, its whole range, and
    an in-between step with its maximum. On a G502 X (100-25600 in steps of 50) these are
    the lists the self-test always used."""
    values = sorted(set(values))
    near = lambda target: min(values, key=lambda dpi: abs(dpi - target))
    common = sorted({near(target) for target in (400, 800, 1200, 1600, 3200, 6400)})
    lo, hi = values[0], values[-1]
    whole_range = sorted({lo, near(400), near(1600), near(6400), hi})[:5]
    in_between = sorted({values[min(len(values) - 1, 15)], hi})
    cases = [common[:1], common[:2], common[:3], common[:4], whole_range, in_between]
    return [case for case in cases if case]

def state_info_rates(serve):
    return serve.result("state")["info"]["report_rates_hz"]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--profile", type=int, default=3)
    parser.add_argument("--name", default="Omalogi Test")
    parser.add_argument("--binary", default="omalogi")
    parser.add_argument("--workdir", help="where to keep the backups; a new temporary directory by default")
    args = parser.parse_args()

    work = args.workdir or tempfile.mkdtemp(prefix="omalogi-selftest-")
    start = os.path.join(work, "start.json")
    cli(args.binary, "backup", "-o", start)

    serve = Serve(args.binary)
    onboard = serve.result("state")["onboard"]
    slot = onboard["profiles"][args.profile - 1]
    if not slot["enabled"] or slot["active"] or slot["profile"]["name"] != args.name:
        serve.close()
        sys.exit(f"profile {args.profile} must be turned on, not in use, and named {args.name!r}")
    home = onboard["active_position"] + 1

    report = Report()
    started = time.monotonic()
    try:
        run(serve, args.binary, args.profile, report)
    except Exception as error:  # noqa: BLE001 - report it, then restore
        report.check("the self-test ran to the end", False, repr(error))
    finally:
        try:
            serve.result("activate", profile=home)
        except Exception as error:  # noqa: BLE001
            report.check(f"activating profile {home} again", False, repr(error))
        serve.close()

    end = os.path.join(work, "end.json")
    cli(args.binary, "backup", "-o", end)
    differences = sector_differences(start, end)
    report.check("profile memory is byte for byte as it was", not differences,
                 f"sectors {differences} differ")
    if differences:
        cli(args.binary, "restore", start)
        again = os.path.join(work, "restored.json")
        cli(args.binary, "backup", "-o", again)
        report.check("a restore from the backup puts it back", not sector_differences(start, again))

    print(f"\n{report.passed} checks passed, {len(report.failed)} failed "
          f"in {time.monotonic() - started:.0f} s")
    for cmd, times in sorted(serve.times.items()):
        print(f"  {cmd:<12} {len(times):>4} requests  median {statistics.median(times):6.0f} ms"
              f"  max {max(times):6.0f} ms")
    for failure in report.failed:
        print(f"FAIL {failure}")
    print(f"backups in {work}")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
