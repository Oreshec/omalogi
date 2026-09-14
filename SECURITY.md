# Security

Omalogi writes to a mouse's onboard memory and installs a udev rule, so bugs in either can
matter beyond a crash. Please report these privately:

- a way to make Omalogi write profile memory it did not verify, or skip its backup;
- anything that could leave a mouse unusable or change its firmware;
- the udev rule granting access to more than the supported mouse's HID++ interface;
- the installer or `omalogi setup` writing outside Omalogi's own files.

## Reporting

Use **Report a vulnerability** on the repository's Security tab. Include the Omalogi
version (`omalogi --version`), the mouse, and the steps to reproduce. You will get an
answer within a week, and a fix is released as soon as it is ready.

Do not post these in public issues. Backups in `$XDG_STATE_HOME/omalogi/backups/`
and raw device dumps contain your mouse's unit ID, so leave them out of reports unless
asked, and never attach them publicly.

## Supported versions

Only the latest release gets fixes.
