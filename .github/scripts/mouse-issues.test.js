const test = require("node:test")
const assert = require("node:assert/strict")
const Issues = require("./mouse-issues.js")

const form = (fields) => Object.entries(fields).map(([question, answer]) => `### ${question}\n\n${answer}`).join("\n\n")

const report = form({
  "What would you like to do?": "Verify it (I will run the 15-minute self-test)",
  "Mouse model": "Logitech G305",
  "USB id or wireless id": "046d:4074",
  "Connection": "Through a receiver (LIGHTSPEED, Bolt or Unifying)",
  "Firmware": "_No response_",
  "How did it go?": "Reading profiles: ok\nChanging DPI levels: ok"
})

test("reads the answers of an issue form", () => {
  const fields = Issues.parseForm(report)
  assert.equal(fields["Mouse model"], "Logitech G305")
  assert.equal(fields["Firmware"], "", "an empty answer is empty")
  assert.match(fields["How did it go?"], /^Reading profiles: ok/)
  assert.deepEqual(Issues.parseForm(""), {})
})

test("finds the product id however it is written", () => {
  for (const text of ["046d:c08b", "C08B", "0xc08b", "046D:C08B (wired)", "usb 046d c08b"]) {
    assert.equal(Issues.productId(text), "c08b", text)
  }
  assert.equal(Issues.productId("0x4074"), "4074")
  assert.equal(Issues.productId("046d"), null, "the vendor id alone names no mouse")
  assert.equal(Issues.productId("G305"), null)
  assert.equal(Issues.productId(undefined), null)
})

test("repeats a model name without markup or mentions", () => {
  assert.equal(Issues.plainName("Logitech G502 X Plus"), "Logitech G502 X Plus")
  assert.equal(Issues.plainName("@someone **G305** #15 [x](y)"), "someone G305 15 xy")
  assert.equal(Issues.plainName("x".repeat(100)).length, 60)
})

test("matches a new report to the open issue for the same mouse", () => {
  const older = { number: 17, body: form({ "Mouse model": "Logitech G502 Hero", "USB id": "046d:c08b" }) }
  const tracking = { number: 15, body: form({ "USB id or wireless id": "c08b" }) }
  const pull = { number: 20, pull_request: {}, body: form({ "USB id or wireless id": "c08b" }) }
  const issues = [tracking, pull, older, { number: 30, body: report }]

  assert.equal(Issues.trackingIssue(issues, "c08b", 31), older, "an older device report counts")
  assert.equal(Issues.trackingIssue(issues, "4074", 31).number, 30)
  assert.equal(Issues.trackingIssue(issues, "4074", 30), null, "never the new issue itself")
  assert.equal(Issues.trackingIssue(issues, "c099", 31), null)
})

test("reads a claim command at the start of a comment", () => {
  assert.equal(Issues.command("/claim"), "claim")
  assert.equal(Issues.command("  /Claim\nI have a G305 on a receiver."), "claim")
  assert.equal(Issues.command("/unclaim"), "unclaim")
  assert.equal(Issues.command("I will /claim this"), null, "only at the start")
  assert.equal(Issues.command("/claimed"), null)
  assert.equal(Issues.command(""), null)
})
