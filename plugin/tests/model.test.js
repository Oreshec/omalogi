// Tests for plugin/Model.js. Run with: node --test plugin/tests

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

function loadModel() {
  const source = fs
    .readFileSync(path.join(__dirname, "..", "Model.js"), "utf8")
    .replace(/^\.pragma library\s*$/m, "")
  const context = vm.createContext({})
  vm.runInContext(source, context)
  return context
}

const Model = loadModel()

function slot(overrides) {
  return {
    position: 1,
    enabled: true,
    active: false,
    profile: {
      name: null,
      report_rate_ms: 1,
      default_dpi_index: 2,
      shift_dpi_index: 0,
      dpi_stages: [800, 1200, 1600, null, 3200]
    },
    ...overrides
  }
}

test("parseJson returns null for unreadable output", () => {
  assert.equal(Model.parseJson("{"), null)
  assert.deepEqual(JSON.parse(JSON.stringify(Model.parseJson('{"a":1}'))), { a: 1 })
})

test("errorMessage keeps omalogi's own explanation", () => {
  const stderr = "omalogi: permission denied opening /dev/hidraw8; install OpenLogi's udev rule\n"
  assert.equal(
    Model.errorMessage(stderr, 1),
    "permission denied opening /dev/hidraw8; install OpenLogi's udev rule"
  )
  assert.equal(Model.errorMessage("", 3), "omalogi exited with status 3.")
  assert.equal(Model.errorMessage("", 127), "The omalogi command is not installed or not on PATH.")
})

test("clampCursor stays inside the list", () => {
  assert.equal(Model.clampCursor(-1, 5), 0)
  assert.equal(Model.clampCursor(7, 5), 4)
  assert.equal(Model.clampCursor(3, 0), 0)
})

test("initialCursor starts on the active profile", () => {
  assert.equal(Model.initialCursor({ active_position: 1 }), 1)
  assert.equal(Model.initialCursor({ active_position: null }), 0)
  assert.equal(Model.initialCursor(null), 0)
})

test("deviceSummary shows the active firmware and live settings", () => {
  const info = {
    name: "G502 X",
    dpi: 1600,
    report_rate_hz: 1000,
    firmware: [
      { version: "BL1 59.00.B0002", active: false },
      { version: "U1 60.00.B0009", active: true }
    ]
  }
  assert.equal(Model.deviceSummary(info), "G502 X  ·  U1 60.00.B0009  ·  1600 DPI  ·  1000 Hz")
})

test("profile titles and status", () => {
  assert.equal(Model.profileTitle(slot({})), "Profile 2")
  assert.equal(Model.profileTitle(slot({ profile: { ...slot({}).profile, name: "Aim" } })), "2  Aim")
  assert.equal(Model.profileStatus(slot({ active: true })), "Active  ·  1000 Hz")
  assert.equal(Model.profileStatus(slot({ enabled: false })), "Disabled on the mouse")
})

test("activationRefusal explains why a profile cannot be activated", () => {
  assert.equal(Model.activationRefusal(slot({})), "")
  assert.equal(Model.activationRefusal(slot({ enabled: false })), "Profile 2 is disabled on the mouse.")
  assert.equal(Model.activationRefusal(slot({ active: true })), "Profile 2 is already active.")
})

test("dpiStages skips unused stages and marks default and shift", () => {
  const stages = JSON.parse(JSON.stringify(Model.dpiStages(slot({}).profile)))
  assert.deepEqual(stages, [
    { dpi: 800, isDefault: false, isShift: true },
    { dpi: 1200, isDefault: false, isShift: false },
    { dpi: 1600, isDefault: true, isShift: false },
    { dpi: 3200, isDefault: false, isShift: false }
  ])
})

test("daemonNote explains automatic switches only for the active profile", () => {
  const daemon = { connected: true, active_profile: 2, source: "rule 1", app: "cs2", error: null }
  assert.equal(Model.daemonNote(daemon, slot({ active: true })), "Set automatically by rule 1 for cs2")
  assert.equal(
    Model.daemonNote({ ...daemon, source: "default" }, slot({ active: true })),
    "Set automatically as the default profile"
  )
  assert.equal(Model.daemonNote(daemon, slot({ active: false })), "")
  assert.equal(Model.daemonNote({ ...daemon, source: null }, slot({ active: true })), "")
  assert.equal(Model.daemonNote({ ...daemon, active_profile: 1 }, slot({ active: true })), "")
  assert.equal(Model.daemonNote(null, slot({ active: true })), "")
})

test("daemonProblem surfaces daemon errors", () => {
  assert.equal(Model.daemonProblem({ error: "rule 2: profile 3 is disabled" }), "Auto-switching: rule 2: profile 3 is disabled")
  assert.equal(Model.daemonProblem({ error: null }), "")
  assert.equal(Model.daemonProblem(null), "")
})

test("boundSlots keeps slot numbers of bound buttons", () => {
  const slots = JSON.parse(JSON.stringify(Model.boundSlots(["left click", null, "DPI up"])))
  assert.deepEqual(slots, [
    { slot: 0, label: "left click" },
    { slot: 2, label: "DPI up" }
  ])
})
