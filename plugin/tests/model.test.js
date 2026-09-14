// Tests for plugin/Model.js. Run with: node --test plugin/tests

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const vm = require("node:vm")

function loadLibrary(file) {
  const source = fs
    .readFileSync(path.join(__dirname, "..", file), "utf8")
    .replace(/^\.pragma library\s*$/m, "")
  const context = vm.createContext({ encodeURIComponent })
  vm.runInContext(source, context)
  return context
}

const Model = loadLibrary("Model.js")
const Icons = loadLibrary("Icons.js")

test("every icon the model names exists, and icons take the theme color", () => {
  const catalogValues = [
    "left", "right", "middle", "back", "forward", "button:7", "dpi-up", "dpi-down", "dpi-cycle",
    "dpi-default", "dpi-shift", "gshift", "profile-next", "profile-previous", "profile-cycle",
    "scroll-left", "scroll-right", "scroll-up", "scroll-down", "media:play-pause", "media:next-track",
    "media:previous-track", "media:volume-up", "media:volume-down", "media:mute", "disabled",
    "key:ctrl+t", "key:", "something-new"
  ]
  for (const action of catalogValues) {
    assert.ok(Icons.SVG[Model.actionIcon(action)] !== undefined, `icon for ${action}`)
  }
  for (const group of ["Mouse", "Keyboard", "Media", "DPI", "Profiles", "Scroll", "Other", "Unknown"]) {
    assert.ok(Icons.SVG[Model.groupIcon(group)] !== undefined, `icon for ${group}`)
  }
  assert.equal(Icons.source("no-such-icon", "#ffffff"), "")
  const source = decodeURIComponent(Icons.source("gauge", { r: 1, g: 0.5, b: 0 }))
  assert.ok(source.startsWith("data:image/svg+xml;utf8,<svg"), source.slice(0, 40))
  assert.ok(source.includes('stroke="#ff8000"'), "the color replaces currentColor")
  assert.ok(!source.includes("%COLOR%"))
})

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
  assert.equal(Model.profileStatus(slot({ active: true })), "In use  ·  1000 Hz")
  assert.equal(
    Model.profileStatus(slot({ active: false })),
    "Not in use  ·  1000 Hz  ·  changes apply when you activate it"
  )
  assert.match(Model.profileStatus(slot({ enabled: false })), /^Turned off on the mouse/)
})

test("activationRefusal explains why a profile cannot be activated", () => {
  assert.equal(Model.activationRefusal(slot({})), "")
  assert.equal(
    Model.activationRefusal(slot({ enabled: false })),
    "Profile 2 is turned off on the mouse, so it can't be activated."
  )
  assert.equal(Model.activationRefusal(slot({ active: true })), "Profile 2 is already active.")
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
  assert.equal(Model.changeCount(original, original), 0)
  assert.equal(Model.draftProblem(original), "")
})

test("changing stages sends the default and shift they point at", () => {
  const original = Model.draftFromSlot(editableSlot())
  const draft = Model.setStage(original, 0, 400)
  assert.equal(draft.shiftDpi, 400, "shift follows the stage it pointed at")
  assert.deepEqual(plain(Model.serveChanges(draft, original)), {
    dpi: [400, 1200, 1600, 2400, 3200],
    default_dpi: 1600,
    shift_dpi: 400
  })
  assert.equal(original.dpiStages[0], 800, "drafts are copied, not mutated")
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

test("openRequest reads profile, tab and button from the payload", () => {
  assert.equal(Model.openRequest(null), null)
  assert.equal(Model.openRequest({}), null)
  assert.equal(Model.openRequest({ profile: 0 }), null)
  assert.deepEqual(plain(Model.openRequest({ profile: 3, tab: "gshift", button: 4 })), {
    profile: 3,
    tab: "gshift",
    button: 4
  })
  assert.deepEqual(plain(Model.openRequest({ tab: "nope", button: 2 })), { profile: null, tab: null, button: 2 })
})

test("canvas helpers index entries and find the ones without a position", () => {
  const entries = [{ slot: 0 }, { slot: 11 }, { slot: 3 }]
  const views = [{ hotspots: [{ slot: 0 }] }, { hotspots: [{ slot: 3 }] }]
  assert.deepEqual(Object.keys(Model.indexBySlot(entries)).sort(), ["0", "11", "3"])
  assert.deepEqual(plain(Model.entriesWithoutHotspot(entries, views)), [{ slot: 11 }])
  assert.deepEqual(plain(Model.entriesWithoutHotspot(entries, [])), entries)
})

test("assignedNames lists the buttons that use an action", () => {
  const entries = [
    { name: "G1", action: "left" },
    { name: "G4", action: "back" },
    { name: "G7", action: "key:ctrl+t" },
    { name: "G8", action: "back" }
  ]
  assert.deepEqual(plain(Model.assignedNames(entries, "back")), ["G4", "G8"])
  assert.deepEqual(plain(Model.assignedNames(entries, "key:")), ["G7"])
  assert.deepEqual(plain(Model.assignedNames(entries, "forward")), [])
  assert.deepEqual(plain(Model.assignedNames(null, "back")), [])
})

test("viewForSlot finds the view that shows a button", () => {
  const views = [{ hotspots: [{ slot: 0 }, { slot: 9 }] }, { hotspots: [{ slot: 3 }] }]
  assert.equal(Model.viewForSlot(views, 9), 0)
  assert.equal(Model.viewForSlot(views, 3), 1)
  assert.equal(Model.viewForSlot(views, 11), -1)
  assert.equal(Model.viewForSlot(null, 0), -1)
})

test("factoryAction reads what use default puts back", () => {
  const onboard = { factory: { buttons: ["left", "right", null], gshift_buttons: ["disabled"] } }
  assert.equal(Model.factoryAction(onboard, "buttons", 1), "right")
  assert.equal(Model.factoryAction(onboard, "buttons", 2), null)
  assert.equal(Model.factoryAction(onboard, "buttons", 20), null)
  assert.equal(Model.factoryAction(onboard, "gshift", 0), "disabled")
  assert.equal(Model.factoryAction({ factory: null }, "buttons", 0), null)
})

test("fitPictureHeight fits the width within bounds", () => {
  const views = [{ width: 1000, height: 2000 }, { width: 500, height: 2000 }]
  // Aspect ratios add up to 0.75, so 300 px of width holds a 400 px tall picture.
  assert.equal(Model.fitPictureHeight(views, 320, 20, 100, 1000), 400)
  assert.equal(Model.fitPictureHeight(views, 320, 20, 100, 300), 300)
  assert.equal(Model.fitPictureHeight(views, 50, 20, 100, 300), 100)
  assert.equal(Model.fitPictureHeight([], 50, 20, 100, 300), 300)
})

test("actionRows puts a header before each section", () => {
  const rows = plain(
    Model.actionRows([
      { group: "Mouse", actions: [{ value: "back", label: "back", group: "Mouse" }] },
      { group: "Media", actions: [{ value: "media:mute", label: "mute", group: "Media" }] }
    ])
  )
  assert.deepEqual(rows.map((row) => row.kind + ":" + (row.value || row.group)), [
    "header:Mouse",
    "action:back",
    "header:Media",
    "action:media:mute"
  ])
})

test("modifiersLabel and buttonName", () => {
  assert.equal(Model.modifiersLabel(QT.CTRL | QT.META), "Ctrl+Super")
  assert.equal(Model.modifiersLabel(0), "")
  assert.equal(Model.buttonName(3, true), "G4")
  assert.equal(Model.buttonName(3, false), "Slot 3")
})

test("slotEntries label changed bindings from the catalog and shortcuts", () => {
  const slot = {
    ...editableSlot(),
    labels: {
      buttons: ["left click", "right click", "middle click", "back", "DPI shift (hold)", "forward", "scroll left"],
      gshift_buttons: []
    }
  }
  const original = Model.draftFromSlot(slot)
  let draft = Model.setBinding(original, "buttons", 3, "key:ctrl+pageup")
  draft = Model.setBinding(draft, "buttons", 5, "media:mute")
  const catalog = [{ value: "media:mute", label: "mute", group: "Media" }]
  const entries = plain(Model.slotEntries(slot, draft, original, catalog, "buttons", 6, true))
  assert.deepEqual(entries.find((entry) => entry.slot === 3), {
    slot: 3,
    name: "G4",
    label: "Ctrl+Page Up",
    changed: true,
    action: "key:ctrl+pageup"
  })
  assert.equal(entries.find((entry) => entry.slot === 5).label, "mute")
  // Slot 6 is past the 6 physical buttons: a bound extra, named by slot.
  assert.equal(entries.find((entry) => entry.slot === 6).name, "Slot 6")
  const left = entries.find((entry) => entry.slot === 0)
  assert.equal(left.changed, false)
  assert.equal(left.label, "left click")
  assert.deepEqual(plain(Model.slotEntries(null, draft, original, catalog, "buttons", 6, true)), [])
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

test("editable slots are physical buttons plus bound extras", () => {
  const original = Model.draftFromSlot(editableSlot())
  // button_count 6: slots 0-5, plus slot 6 which is bound to scroll-left.
  assert.deepEqual(plain(Model.editableSlots(original, "buttons", 6)), [0, 1, 2, 3, 4, 5, 6])
  assert.deepEqual(plain(Model.editableSlots(original, "gshift", 2)), [0, 1, 2])
})

test("every keyboard shortcut is the picker's one shortcut entry", () => {
  assert.equal(Model.actionChoice("key:ctrl+t"), "key:")
  assert.equal(Model.actionChoice("back"), "back")
  assert.equal(Model.actionChoice(null), "")
  assert.equal(Model.keyCombo("key:ctrl+t"), "ctrl+t")
  assert.equal(Model.keyCombo("back"), "")
})

const QT = { SHIFT: 0x02000000, CTRL: 0x04000000, ALT: 0x08000000, META: 0x10000000 }
const scan = (evdev) => evdev + 8

test("recordKey turns physical keys into shortcuts", () => {
  assert.deepEqual(plain(Model.recordKey(scan(20), QT.CTRL | QT.SHIFT)), {
    combo: "ctrl+shift+t",
    waiting: false,
    unsupported: false
  })
  // The physical key, whatever Shift would type on the layout.
  assert.equal(Model.recordKey(scan(2), QT.SHIFT).combo, "shift+1")
  assert.equal(Model.recordKey(scan(183), QT.META | QT.ALT).combo, "alt+super+f13")
  assert.equal(Model.recordKey(scan(104), 0).combo, "pageup")
  assert.deepEqual(plain(Model.recordKey(scan(29), QT.CTRL)), { combo: "", waiting: true, unsupported: false })
  assert.deepEqual(plain(Model.recordKey(scan(240), 0)), { combo: "", waiting: false, unsupported: true })
})

test("comboLabel shows shortcuts like the mouse's labels", () => {
  assert.equal(Model.comboLabel("ctrl+shift+tab"), "Ctrl+Shift+Tab")
  assert.equal(Model.comboLabel("super+f13"), "Super+F13")
  assert.equal(Model.comboLabel("ctrl+pageup"), "Ctrl+Page Up")
  assert.equal(Model.comboLabel("alt+backslash"), "Alt+\\")
  assert.equal(Model.comboLabel(""), "")
})

test("actionSections groups in display order and filters", () => {
  const catalog = [
    { value: "dpi-up", label: "DPI up", group: "DPI" },
    { value: "back", label: "back", group: "Mouse" },
    { value: "media:mute", label: "mute", group: "Media" },
    { value: "key:", label: "Keyboard shortcut…", group: "Keyboard" },
    { value: "custom", label: "custom", group: "Extra" }
  ]
  assert.deepEqual(
    plain(Model.actionSections(catalog, "")).map((section) => section.group),
    ["Mouse", "Keyboard", "Media", "DPI", "Extra"]
  )
  assert.deepEqual(plain(Model.actionSections(catalog, "  DPI ")), [
    { group: "DPI", actions: [{ value: "dpi-up", label: "DPI up", group: "DPI" }] }
  ])
  assert.deepEqual(
    plain(Model.actionSections(catalog, "shortcut")).map((section) => section.group),
    ["Keyboard"]
  )
  assert.deepEqual(plain(Model.actionSections(catalog, "nothing like this")), [])
  assert.deepEqual(plain(Model.actionGroups(catalog)), ["Mouse", "Keyboard", "Media", "DPI", "Extra"])
})

test("calloutLayout puts cards beside their buttons without overlap", () => {
  const views = [
    {
      width: 1000, height: 2000, image: "/front.png",
      hotspots: [
        { slot: 0, x: 0.3, y: 0.2 },
        { slot: 9, x: 0.2, y: 0.22 },
        { slot: 1, x: 0.8, y: 0.2 },
        { slot: 2, x: 0.5, y: 0.3 }
      ]
    },
    { width: 500, height: 2000, image: "/side.png", hotspots: [{ slot: 4, x: 0.2, y: 0.5 }] }
  ]
  const layout = plain(Model.calloutLayout(views, 400, 20, 40, 8))
  assert.equal(layout.picturesWidth, 200 + 20 + 100)
  assert.equal(layout.height, 400)
  assert.deepEqual(layout.left.map((card) => card.slot), [0, 9])
  assert.deepEqual(layout.right.map((card) => card.slot), [1, 2, 4])
  for (const side of [layout.left, layout.right]) {
    side.forEach((card, i) => {
      assert.ok(card.cardY >= 0 && card.cardY <= 400 - 40, `card ${card.slot} inside`)
      if (i > 0) assert.ok(card.cardY >= side[i - 1].cardY + 48, `card ${card.slot} clear of the one above`)
    })
  }
  // The side view's button sits at its own offset.
  assert.equal(layout.right[2].x, 220 + 0.2 * 100)
  // Too many cards for the height: stacked, and the canvas grows.
  const crowded = [{ width: 100, height: 100, image: "/x.png", hotspots: [0, 1, 2, 3, 4, 5].map((slot) => ({ slot, x: 0.9, y: 0.5 })) }]
  const tall = plain(Model.calloutLayout(crowded, 100, 0, 30, 6))
  assert.deepEqual(tall.right.map((card) => card.cardY), [0, 36, 72, 108, 144, 180])
  assert.equal(tall.height, 210)
})

test("changedSlots and changeCount track unsaved edits", () => {
  const original = Model.draftFromSlot(editableSlot())
  assert.equal(Model.changeCount(original, original), 0)
  let draft = Model.setBinding(original, "buttons", 3, "key:ctrl+t")
  draft = Model.setBinding(draft, "gshift", 1, "media:mute")
  draft = Model.setField(draft, "rateHz", 500)
  assert.deepEqual(plain(Model.changedSlots(draft, original, "buttons")), [3])
  assert.deepEqual(plain(Model.changedSlots(draft, original, "gshift")), [1])
  assert.equal(Model.changeCount(draft, original), 3)
  assert.equal(Model.changeCount(null, original), 0)
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

test("the DPI slider is logarithmic and snaps to the sensor's step", () => {
  const bounds = { min: 100, max: 25600, step: 50 }
  assert.equal(Model.dpiToPosition(100, bounds), 0)
  assert.equal(Model.dpiToPosition(25600, bounds), 1000)
  // Each doubling gets the same travel: 8 doublings from 100 to 25600.
  assert.equal(Model.dpiToPosition(1600, bounds), 500)
  for (const dpi of [400, 800, 1600, 3200, 6400]) {
    assert.equal(Model.positionToDpi(Model.dpiToPosition(dpi, bounds), bounds), dpi)
  }
  assert.equal(Model.positionToDpi(0, bounds), 100)
  assert.equal(Model.positionToDpi(1000, bounds), 25600)
  assert.equal(Model.positionToDpi(437, bounds) % 50, 0)
})

test("serveChanges sends only what changed", () => {
  const original = Model.draftFromSlot(editableSlot())
  assert.deepEqual(plain(Model.serveChanges(original, original)), {})
  let draft = Model.setField(original, "rateHz", 500)
  draft = Model.setBinding(draft, "buttons", 6, "key:ctrl+a")
  draft = Model.setBinding(draft, "gshift", 2, "media:mute")
  assert.deepEqual(plain(Model.serveChanges(draft, original)), {
    rate: 500,
    buttons: { 6: "key:ctrl+a" },
    gshift: { 2: "media:mute" }
  })
  // Stage edits carry the default and shift stages with them.
  assert.deepEqual(plain(Model.serveChanges(Model.setStage(original, 4, 6400), original)), {
    dpi: [800, 1200, 1600, 2400, 6400],
    default_dpi: 1600,
    shift_dpi: 800
  })
  assert.deepEqual(plain(Model.serveChanges(Model.setField(original, "defaultDpi", 2400), original)), {
    default_dpi: 2400
  })
})

test("saveStatus reports whether the mouse uses the change", () => {
  const reply = (takes_effect) => ({ slot: { position: 1 }, takes_effect, undo: 1 })
  assert.deepEqual(plain(Model.saveStatus(reply({ state: "now" }), false)), {
    text: "Saved to profile 2. The mouse is using it now.",
    isError: false
  })
  assert.equal(
    Model.saveStatus(reply({ state: "when_activated" }), true).text,
    "Undid the last change to profile 2. It applies when you activate this profile."
  )
  const notLoaded = Model.saveStatus(reply({ state: "not_loaded", reason: "no other profile is enabled" }), false)
  assert.equal(notLoaded.isError, true)
  assert.match(notLoaded.text, /no other profile is enabled\.$/)
  assert.equal(Model.saveStatus(reply(null), false).text, "Profile 2 already has these settings.")
})

test("withSlot and withActive update profiles without a full read", () => {
  const onboard = {
    mode: "onboard",
    description: {},
    active_position: 0,
    profiles: [0, 1, 2].map((position) => ({ position, active: position === 0, profile: { v: 1 } }))
  }
  const saved = plain(Model.withSlot(onboard, { position: 1, active: false, profile: { v: 2 } }))
  assert.deepEqual(saved.profiles.map((slot) => slot.profile.v), [1, 2, 1])
  assert.equal(saved.active_position, 0)
  const switched = plain(Model.withActive(onboard, 2))
  assert.deepEqual(switched.profiles.map((slot) => slot.active), [false, false, true])
  assert.equal(switched.active_position, 2)
  assert.equal(onboard.profiles[0].active, true, "the original is not mutated")
})

test("the DPI bar places levels, ticks and roles", () => {
  const bounds = { min: 100, max: 25600, step: 50 }
  const draft = Model.draftFromSlot(editableSlot())
  const nodes = plain(Model.dpiNodes(draft, bounds))
  assert.deepEqual(nodes.map((node) => node.dpi), [800, 1200, 1600, 2400, 3200])
  assert.equal(nodes[2].isDefault, true)
  assert.equal(nodes[0].isShift, true)
  assert.equal(nodes[2].fraction, 0.5)
  assert.deepEqual(
    plain(Model.dpiTicks(bounds)).map((tick) => tick.dpi),
    [100, 200, 400, 800, 1600, 3200, 6400, 12800, 25600]
  )
  assert.deepEqual(plain(Model.dpiTicks({ min: 200, max: 12000, step: 50 })).map((tick) => tick.dpi).slice(-2), [6400, 12000])
  assert.equal(Model.dpiAtFraction(0.5, bounds), 1600)
  assert.equal(Model.dpiAtFraction(-1, bounds), 100)
})

test("levels stay in order and keep their roles", () => {
  const original = Model.draftFromSlot(editableSlot())
  const four = Model.removeStage(original, 4)
  assert.deepEqual(plain(Model.insertStage(four, 1000).dpiStages), [800, 1000, 1200, 1600, 2400])
  assert.equal(Model.insertStage(original, 1000), original, "at most five levels")
  // Dragging level 1 past level 3 re-sorts; the default keeps its DPI value.
  const moved = Model.sortStages(Model.setStage(original, 0, 2000))
  assert.deepEqual(plain(moved.dpiStages), [1200, 1600, 2000, 2400, 3200])
  assert.equal(moved.defaultDpi, 1600)
  assert.equal(moved.shiftDpi, 2000, "shift followed its level")
  // Removing the default level hands the role to the nearest level.
  const removed = Model.removeStageKeepingRoles(original, 2)
  assert.deepEqual(plain(removed.dpiStages), [800, 1200, 2400, 3200])
  assert.equal(removed.defaultDpi, 1200)
  assert.equal(removed.shiftDpi, 800)
  assert.equal(Model.draftProblem(removed), "")
})

test("gshiftButtons names the buttons that reach the G-Shift layer", () => {
  const slot = editableSlot()
  const base = Model.draftFromSlot(slot)
  const none = { ...base, buttons: base.buttons.map((action) => (action === "gshift" ? "left" : action)) }
  const holding = (index) => ({ ...none, buttons: none.buttons.map((action, i) => (i === index ? "gshift" : action)) })
  assert.deepEqual(plain(Model.gshiftButtons(none, 11)), [])
  assert.deepEqual(plain(Model.gshiftButtons(holding(4), 11)), ["G5"])
  assert.deepEqual(plain(Model.gshiftButtons(holding(12), 11)), [], "a slot past the physical buttons cannot be pressed")
  assert.deepEqual(plain(Model.gshiftButtons(null, 11)), [])
})

test("the setup screen names the one step that reaches the mouse", () => {
  assert.equal(Model.setupState(""), "")
  assert.equal(Model.setupState(Model.errorMessage("", 127)), "helper")
  assert.equal(
    Model.setupState("permission denied opening /dev/hidraw8; install Omalogi's udev rule (packaging/udev/70-omalogi.rules) and replug the mouse"),
    "access"
  )
  assert.equal(
    Model.setupState("no supported Logitech mouse found; plug a G-series mouse in over USB, or turn on a wireless one paired to its receiver"),
    "device"
  )
  assert.equal(
    Model.setupState("no supported Logitech device found; is the G502 X (046d:c099) plugged in over USB?"),
    "device",
    "helpers before 0.2 said device"
  )
  assert.equal(Model.setupState("device request failed"), "error")
  assert.equal(Model.setupCopy("helper").action, "Install helper")
  assert.equal(Model.setupCopy("access").action, "Allow access")
  assert.equal(Model.setupCopy("outdated").action, "Update helper")
  assert.equal(Model.setupCopy("device").action, "")
  assert.equal(Model.setupCopy("error", "device request failed").body, "device request failed")
  assert.equal(Model.helperOutdated({ info: {}, onboard: {} }), true, "helpers that do not report a protocol speak 1, older than this plugin needs")
  assert.equal(Model.helperOutdated({ helper: { version: "0.1.9", protocol: 1 } }), true)
  assert.equal(Model.helperOutdated({ helper: { version: "0.2.0", protocol: 2 } }), false)
  assert.equal(Model.helperOutdated({ helper: { version: "0.0.1", protocol: 0.5 } }), true)
  assert.equal(Model.helperInstallCommand("/home/me/.config/omarchy/plugins/io.github.elberacasa.omalogi/install.sh"),
    "bash '/home/me/.config/omarchy/plugins/io.github.elberacasa.omalogi/install.sh'")
  assert.equal(Model.helperInstallCommand("/tmp/it's here/install.sh"), "bash '/tmp/it'\\''s here/install.sh'")
  assert.deepEqual(plain(Model.helperInstallArgv("/p/install.sh")),
    ["omarchy-launch-floating-terminal-with-presentation", "bash '/p/install.sh'"])
  assert.equal(Model.localPath("file:///home/me/My%20Plugins/install.sh"), "/home/me/My Plugins/install.sh")
  assert.doesNotMatch(Model.helperInstallCommand("/p/install.sh"), /curl|wget/, "never a download piped to a shell")
  assert.match(Model.PLUGIN_ADD_COMMAND, /^omarchy plugin add https:\/\/github\.com\/elberacasa\/omalogi --enable$/)
})

test("untested mice get a badge, one acceptance and a report link", () => {
  assert.deepEqual(plain(Model.support({ info: {}, onboard: {} })), { name: "G502 X", verified: true, editable: true, accepted: true },
    "helpers before protocol 2 only opened the verified G502 X")
  const verified = { name: "G502 X", verified: true, editable: true, accepted: true, memory_model: 1, profile_format: 4 }
  const untested = { name: "G502 Hero", verified: false, editable: true, accepted: false, memory_model: 1, profile_format: 4 }
  const unreadable = { name: "G502 Hero", verified: false, editable: false, accepted: false, memory_model: 1, profile_format: 6 }
  assert.equal(Model.support({ support: untested }), untested)
  assert.equal(Model.supportBadge(verified), null)
  assert.equal(Model.supportBadge(untested).text, "Untested")
  assert.match(Model.supportBadge(untested).detail, /G502 Hero/)
  assert.equal(Model.supportBadge(unreadable).text, "Not supported yet")
  assert.equal(Model.needsAcceptance(verified), false)
  assert.equal(Model.needsAcceptance(untested), true)
  assert.equal(Model.needsAcceptance({ ...untested, accepted: true }), false)
  assert.equal(Model.needsAcceptance(unreadable), false, "nothing to accept when it cannot be edited")
  const url = Model.reportUrl({ name: "G502 Hero", vendor_id: 0x046d, product_id: 0xc08b }, untested)
  assert.equal(url, "https://github.com/elberacasa/omalogi/issues/new?template=device.yml&title=%5BDevice%5D%3A%20G502%20Hero&model=Logitech%20G502%20Hero&usb=046d%3Ac08b")
  assert.doesNotMatch(url, /unit|serial|backup/i)
})
