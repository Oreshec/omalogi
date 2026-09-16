# Prompt: add a device

```text
Read docs/agents/agent-guide.md and CONTRIBUTING.md ("Adding a device") first and follow their rules.

I own a <model, e.g. Logitech G305> (USB id <046d:xxxx>, connected <wired / LIGHTSPEED
receiver>) and want Omalogi to support it.

1. Before any code, compare its HID++ feature list and onboard profile description with
   libratbag and Solaar data for this device, and tell me what you expect to differ from
   the G502 X. Cite the files you used.
2. Give me the exact read-only commands to dump the mouse with
   research/tools/probe_readonly.py (and the setfacl command for this session). Wait for
   me to paste the output path or results. Do not ask me to run anything that writes.
3. From the dump, check each user profile sector byte by byte, and tell me what to press
   on the mouse to confirm DPI stages and bindings. Wait for my results.
4. Create a redacted fixture in tests/fixtures/ with the unit ID zeroed in device_info,
   add emulator tests, then add the device to SUPPORTED_DEVICES, VERIFIED_LAYOUTS (only if
   the layout is new and verified) and the udev rule.
5. Give me the dry-run, backup, write and restore commands for one hardware write, wait
   for my results, and log them in docs/hardware-tests.md.
6. Run every check in docs/agents/agent-guide.md, then list the device as tested in the README.

Never commit the raw dump, never flash firmware, never write ROM sectors. If something
cannot be verified, stop and tell me what is missing.
```
