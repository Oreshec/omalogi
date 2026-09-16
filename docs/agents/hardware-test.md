# Prompt: test on real hardware

```text
Read docs/agents/agent-guide.md and docs/hardware-tests.md first and follow their rules.

The change to verify: <branch or pull request, and what it changes on the mouse>.
My mouse: <model, USB id, firmware from `omalogi --json info`>.

Prepare a test plan that I run, one step at a time:
1. A full backup: `omalogi backup`, and where it is saved.
2. A dedicated test profile, so my own profiles are never edited: which slot to turn on
   with `omalogi profiles enable N` and name with `omalogi profiles edit N --name
   "Omalogi Test"`.
3. For each write: the --dry-run first, then the write, then how to confirm it on the
   mouse itself (for example the live DPI with `omalogi dpi`).
4. If the change touches the overlay's saving, `scripts/hardware-selftest.py --profile N`.
5. How to restore everything: `omalogi restore <backup>` and turning the test profile off.

Wait for my results after each step. If anything reads back differently than expected,
stop and help me restore before continuing. Then add an entry to docs/hardware-tests.md
with the date, device, firmware, what was checked and the result. Do not include the
mouse's unit ID or backup contents.
```
