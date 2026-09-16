# Prompt: verify my mouse

For owners of a mouse Omalogi lists as untested. It takes about 15 minutes, runs only on
a spare profile slot, restores everything at the end, and makes the model verified for
everyone. You run every command; your agent explains and checks.

```text
Read docs/agents/agent-guide.md, CONTRIBUTING.md and docs/hardware-tests.md first and
follow their hard rules.

I want to verify my Logitech <model> (<wired over USB / through its receiver>) with Omalogi.
Guide me one step at a time and wait for my output after each step:

1. `omalogi --version` and `omalogi --json info`: confirm the mouse is found, and note its
   name, USB id or WPID, and firmware. Never ask for or record its unit ID.
2. `omalogi backup`: note where the backup was saved. Never ask me to share its contents.
3. `omalogi profiles`: pick a profile slot that is turned off and not in use. Then
   `omalogi accept-untested`, `omalogi profiles enable N` and
   `omalogi profiles edit N --name "Omalogi Test"`.
4. `scripts/hardware-selftest.py --profile N` from this checkout, and read me its summary.
   It changes only that slot while it is not in use, and puts all profile memory back
   byte for byte at the end.
5. `omalogi profiles disable N` to turn the test profile off again.
6. If anything failed or read back wrong: stop, help me run `omalogi restore <backup>`,
   and help me open a bug report with the self-test summary instead of a pull request.
7. If every check passed, open a pull request that:
   - sets `verified` for this exact model and connection in src/hidraw.rs
     (SUPPORTED_DEVICES for USB, WIRELESS_DEVICES for a receiver), and adds its profile
     layout to VERIFIED_LAYOUTS in src/onboard/format.rs if it is not there yet, citing
     the self-test as the evidence;
   - adds an entry to docs/hardware-tests.md with the date, model, USB id or WPID,
     firmware, profile layout, and the self-test summary (no unit ID, no backups);
   - moves the model to the verified list in the README;
   - passes every check in docs/agents/agent-guide.md.
```
