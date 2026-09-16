# Prompt: fix a bug

```text
Read docs/agents/agent-guide.md first and follow its rules.

The bug: <what happens, what should happen, steps to reproduce, and the output of
`omalogi --version` and `omalogi --json info` if it involves the mouse>.
Issue: <link, if any>.

1. Find the cause and explain it with file:line references before changing anything.
2. Write a failing test that reproduces it: tests/ for helper behaviour (the emulated
   G502 X needs no mouse), plugin/tests/ for overlay logic in Model.js.
3. Make the smallest fix that passes the test, matching the surrounding code.
4. Run every check in docs/agents/agent-guide.md and show me the results.
5. If the fix changes what is written to a mouse, give me hardware-test.md steps to
   confirm it on real hardware before we merge.
```
