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

test("indicator shows the active profile and why", () => {
  const byRule = { connected: true, active_profile: 2, source: "rule 1", app: "cs2", error: null }
  assert.equal(Model.indicatorText(byRule), "󰍽 2")
  assert.equal(Model.indicatorActive(byRule), true)
  assert.equal(Model.indicatorTooltip(byRule), "Omalogi: profile 2 (rule 1 for cs2)")

  const byDefault = { ...byRule, source: "default" }
  assert.equal(Model.indicatorActive(byDefault), false)
  assert.equal(Model.indicatorTooltip(byDefault), "Omalogi: profile 2 (default)")

  const manual = { ...byRule, source: null }
  assert.equal(Model.indicatorTooltip(manual), "Omalogi: profile 2")
})

test("indicator explains missing daemon, device and errors", () => {
  assert.equal(Model.indicatorText(null), "󰍽")
  assert.equal(Model.indicatorTooltip(null), "Omalogi: daemon not running")
  const unplugged = { connected: false, active_profile: null, source: null, error: null }
  assert.equal(Model.indicatorText(unplugged), "󰍽")
  assert.equal(Model.indicatorTooltip(unplugged), "Omalogi: no mouse connected")
  assert.equal(
    Model.indicatorTooltip({ ...unplugged, error: "no supported Logitech device found" }),
    "Omalogi: no supported Logitech device found"
  )
})

function editableSlot() {
  return {
    position: 1,
    enabled: true,
    active: true,
    profile: {
      name: null,
      report_rate_ms: 1,
      default_dpi_index: 2,
      shift_dpi_index: 0,
      dpi_stages: [800, 1200, 1600, 2400, 3200]
    },
    actions: {
      buttons: ["left", "right", "middle", "back", "gshift", "forward", "scroll-left", null],
      gshift_buttons: [null, null, "key:ctrl+t", null, null, null, null, null]
    }
  }
}

const plain = (value) => JSON.parse(JSON.stringify(value))

test("draftFromSlot uses the terms profiles edit accepts", () => {
  const draft = plain(Model.draftFromSlot(editableSlot()))
  assert.deepEqual(draft, {
    number: 2,
    dpiStages: [800, 1200, 1600, 2400, 3200],
    defaultDpi: 1600,
    shiftDpi: 800,
    rateHz: 1000,
    buttons: ["left", "right", "middle", "back", "gshift", "forward", "scroll-left", null],
    gshift: [null, null, "key:ctrl+t", null, null, null, null, null]
  })
})

test("an untouched draft has no changes", () => {
  const original = Model.draftFromSlot(editableSlot())
  assert.equal(Model.hasChanges(original, original), false)
  assert.equal(Model.draftProblem(original), "")
})

test("editArgs lists only what changed", () => {
  const original = Model.draftFromSlot(editableSlot())
  let draft = Model.setField(original, "rateHz", 500)
  draft = Model.setBinding(draft, "buttons", 6, "key:ctrl+a")
  draft = Model.setBinding(draft, "gshift", 2, "media:mute")
  assert.deepEqual(plain(Model.editArgs(draft, original, true)), [
    "profiles", "edit", "2", "--rate", "500",
    "--button", "6=key:ctrl+a", "--gshift", "2=media:mute", "--dry-run"
  ])
  assert.equal(original.rateHz, 1000, "drafts are copied, not mutated")
})

test("changing stages sends the default and shift they point at", () => {
  const original = Model.draftFromSlot(editableSlot())
  const draft = Model.setStage(original, 0, 400)
  assert.equal(draft.shiftDpi, 400, "shift follows the stage it pointed at")
  assert.deepEqual(plain(Model.editArgs(draft, original, false)), [
    "profiles", "edit", "2",
    "--dpi", "400,1200,1600,2400,3200", "--default-dpi", "1600", "--shift-dpi", "400"
  ])
})

test("removing the default stage asks for a new one", () => {
  const original = Model.draftFromSlot(editableSlot())
  let draft = Model.removeStage(original, 2)
  assert.deepEqual(plain(draft.dpiStages), [800, 1200, 2400, 3200])
  assert.equal(draft.defaultDpi, null)
  assert.equal(Model.draftProblem(draft), "Choose the default DPI stage.")
  draft = Model.setField(draft, "defaultDpi", 1200)
  assert.equal(Model.draftProblem(draft), "")
})

test("stages are capped at five and must not be empty", () => {
  let draft = Model.draftFromSlot(editableSlot())
  draft = Model.addStage(draft, 6400)
  assert.equal(draft.dpiStages.length, 5)
  for (let i = 0; i < 5; i++) draft = Model.removeStage(draft, 0)
  assert.equal(Model.draftProblem(draft), "Add at least one DPI stage.")
})

test("openRequest reads profile, edit and tab from the payload", () => {
  assert.equal(Model.openRequest(null), null)
  assert.equal(Model.openRequest({}), null)
  assert.equal(Model.openRequest({ profile: 0 }), null)
  assert.deepEqual(plain(Model.openRequest({ profile: 3, edit: true, tab: "buttons" })), {
    profile: 3,
    edit: true,
    tab: "buttons"
  })
  assert.deepEqual(plain(Model.openRequest({ edit: true, tab: "nope" })), { profile: null, edit: true, tab: "dpi" })
})

test("an unfinished keyboard shortcut blocks writing", () => {
  const draft = Model.setBinding(Model.draftFromSlot(editableSlot()), "gshift", 3, "key:")
  assert.equal(Model.draftProblem(draft), "Type the keyboard shortcut for G-Shift slot 3.")
})

test("dpi bounds and new stages come from the sensor list", () => {
  const info = { dpi_values: [100, 150, 200, 25600] }
  assert.deepEqual(plain(Model.dpiBounds(info)), { min: 100, max: 25600, step: 50 })
  assert.deepEqual(plain(Model.dpiBounds(null)), { min: 100, max: 25600, step: 50 })
  const bounds = Model.dpiBounds(info)
  const draft = Model.draftFromSlot(editableSlot())
  assert.equal(Model.nextStageDpi(draft, bounds), 6400)
  assert.equal(Model.nextStageDpi(Model.setStage(draft, 4, 20000), bounds), 25600)
})

test("dropdown options for stages and rates", () => {
  const draft = Model.draftFromSlot(editableSlot())
  assert.deepEqual(plain(Model.stageOptions(draft))[0], { value: "800", label: "800 DPI" })
  assert.deepEqual(plain(Model.rateOptions({ report_rates_hz: [125, 1000] })), [
    { value: "125", label: "125 Hz" },
    { value: "1000", label: "1000 Hz" }
  ])
})

test("editable slots are physical buttons plus bound extras", () => {
  const original = Model.draftFromSlot(editableSlot())
  // button_count 6: slots 0-5, plus slot 6 which is bound to scroll-left.
  assert.deepEqual(plain(Model.editableSlots(original, "buttons", 6)), [0, 1, 2, 3, 4, 5, 6])
  assert.deepEqual(plain(Model.editableSlots(original, "gshift", 2)), [0, 1, 2])
})

test("action picker groups key combos and keeps unknown current bindings", () => {
  const catalog = [
    { value: "back", label: "back", group: "Mouse" },
    { value: "key:", label: "Keyboard shortcut…", group: "Keyboard" }
  ]
  assert.equal(Model.actionChoice("key:ctrl+t"), "key:")
  assert.equal(Model.keyCombo("key:ctrl+t"), "ctrl+t")
  assert.equal(Model.keyCombo("back"), "")
  assert.equal(plain(Model.actionOptions(catalog, "key:ctrl+t")).length, 2)
  assert.deepEqual(plain(Model.actionOptions(catalog, "button:7"))[0], {
    value: "button:7",
    label: "button:7",
    description: "Current"
  })
  assert.deepEqual(plain(Model.actionOptions(catalog, "back"))[0], {
    value: "back",
    label: "back",
    description: "Mouse"
  })
})

test("pictureViews keeps only usable views", () => {
  assert.deepEqual(plain(Model.pictureViews(null)), [])
  assert.deepEqual(plain(Model.pictureViews({})), [])
  const picture = {
    views: [
      { name: "front", image: "/cache/front.png", width: 1556, height: 2800, hotspots: [] },
      { name: "no-path", image: 3, width: 10, height: 10, hotspots: [] },
      { name: "flat", image: "/cache/flat.png", width: 10, height: 0, hotspots: [] }
    ]
  }
  assert.deepEqual(
    plain(Model.pictureViews(picture)).map((view) => view.name),
    ["front"]
  )
  assert.equal(Model.viewWidth(picture.views[0], 280), 156)
})

test("bindingRows pairs button and G-Shift labels by slot", () => {
  assert.deepEqual(plain(Model.bindingRows(null)), [])
  assert.deepEqual(
    plain(Model.bindingRows({ buttons: ["left click", null, "back", null], gshift_buttons: [null, "mute"] })),
    [
      { slot: 0, button: "left click", gshift: null },
      { slot: 1, button: null, gshift: "mute" },
      { slot: 2, button: "back", gshift: null }
    ]
  )
})

test("boundSlots keeps slot numbers of bound buttons", () => {
  const slots = JSON.parse(JSON.stringify(Model.boundSlots(["left click", null, "DPI up"])))
  assert.deepEqual(slots, [
    { slot: 0, label: "left click" },
    { slot: 2, label: "DPI up" }
  ])
})
