# Prompt: verify my mouse

For owners of a mouse Omalogi lists as untested. It takes about 15 minutes, runs only on
a spare profile slot, restores everything at the end, and makes the model verified for
everyone. You run every command; your agent explains and checks.

The quickest start is one sentence to your coding agent:

> Clone https://github.com/elberacasa/omalogi and follow docs/agents/verify-my-mouse.md
> to verify my mouse.

## Claim the model first

Each model has one issue, listed in [#15](https://github.com/elberacasa/omalogi/issues/15).
The issue's assignee is the person verifying it, so two owners never test the same model.

Agents: run step 1 below first and take the model, its connection, product id and active
firmware from `omalogi --json info` instead of asking the owner to fill in the
placeholders. Then search for the model's issue with
`gh issue list --label verify-a-mouse --state open --search <product id>`, or on
https://github.com/elberacasa/omalogi/issues?q=label%3Averify-a-mouse+is%3Aopen if `gh`
is not set up.

- **The model has an issue with no assignee:** the owner comments `/claim` there and is
  assigned. `/unclaim` gives it back.
- **The model has an issue with an assignee:** someone is already on it. Stop, unless the
  claim has had no update for 30 days; then the owner asks in that issue.
- **The model has no issue:** give the owner this link, with the values filled in and
  URL-encoded, to open the Mouse report form and choose "Verify it". The model is
  assigned to them as soon as the issue is opened.
  `https://github.com/elberacasa/omalogi/issues/new?template=mouse.yml&title=[Mouse]:+<model>&model=Logitech+<model>&device_id=046d:<product id>&firmware=<firmware>`

Never open or comment on issues for the owner without asking; it posts under their account.
Keep the issue number for the pull request.

## The verification

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
   and help me post the self-test summary on the model's issue instead of opening a pull
   request.
7. If every check passed, open a pull request that:
   - sets `verified` for this exact model and connection in src/hidraw.rs
     (SUPPORTED_DEVICES for USB, WIRELESS_DEVICES for a receiver), and adds its profile
     layout to VERIFIED_LAYOUTS in src/onboard/format.rs if it is not there yet, citing
     the self-test as the evidence;
   - adds an entry to docs/hardware-tests.md with the date, model, USB id or WPID,
     firmware, profile layout, and the self-test summary (no unit ID, no backups);
   - moves the model to the verified list in the README;
   - says `Closes #<issue>` for the model's issue;
   - passes every check in docs/agents/agent-guide.md.
```
