.pragma library

// Pure helpers for the Omalogi overlay. No QML types here, so node can test them
// (plugin/tests/model.test.js).

function parseJson(text) {
  try {
    return JSON.parse(text)
  } catch (e) {
    return null
  }
}

// A readable message for a failed omalogi run: its last stderr line without the
// "omalogi: " prefix, which already names the problem and the fix.
function errorMessage(stderr, exitCode) {
  if (exitCode === 127) return "The omalogi command is not installed or not on PATH."
  var lines = String(stderr || "").split("\n").filter(function(line) { return line.trim() !== "" })
  if (lines.length === 0) return "omalogi exited with status " + exitCode + "."
  return lines[lines.length - 1].replace(/^omalogi: /, "")
}

function clampCursor(cursor, count) {
  if (count <= 0) return 0
  return Math.max(0, Math.min(count - 1, cursor))
}

// Start on the active profile when there is one.
function initialCursor(onboard) {
  if (!onboard || onboard.active_position === null || onboard.active_position === undefined) return 0
  return onboard.active_position
}

function activeFirmware(info) {
  var active = (info.firmware || []).filter(function(fw) { return fw.active })
  return active.length > 0 ? active[0].version : ""
}

function deviceSummary(info) {
  var parts = [info.name]
  var firmware = activeFirmware(info)
  if (firmware !== "") parts.push(firmware)
  parts.push(info.dpi + " DPI")
  if (info.report_rate_hz) parts.push(info.report_rate_hz + " Hz")
  return parts.join("  ·  ")
}

function profileTitle(slot) {
  var number = slot.position + 1
  return slot.profile.name ? number + "  " + slot.profile.name : "Profile " + number
}

function reportRate(profile) {
  return profile.report_rate_ms > 0 ? Math.round(1000 / profile.report_rate_ms) + " Hz" : "Unknown rate"
}

function profileStatus(slot) {
  if (!slot.enabled) return "Disabled on the mouse"
  return (slot.active ? "Active" : "Enabled") + "  ·  " + reportRate(slot.profile)
}

// Why a profile cannot be activated, or "" when it can.
function activationRefusal(slot) {
  var number = slot.position + 1
  if (!slot.enabled) return "Profile " + number + " is disabled on the mouse."
  if (slot.active) return "Profile " + number + " is already active."
  return ""
}

// Why the daemon activated this profile, or "" when it did not (or is not running).
function daemonNote(daemon, slot) {
  if (!daemon || !daemon.connected || !daemon.source || !slot.active) return ""
  if (daemon.active_profile !== slot.position + 1) return ""
  if (daemon.source === "default") return "Set automatically as the default profile"
  return "Set automatically by " + daemon.source + (daemon.app ? " for " + daemon.app : "")
}

// A problem the daemon reports, ready for the footer, or "".
function daemonProblem(daemon) {
  return daemon && daemon.error ? "Auto-switching: " + daemon.error : ""
}

var MOUSE_GLYPH = "󰍽"

// Bar indicator text: the mouse glyph, plus the active profile when the daemon knows it.
function indicatorText(daemon) {
  if (!daemon || !daemon.connected || !daemon.active_profile) return MOUSE_GLYPH
  return MOUSE_GLYPH + " " + daemon.active_profile
}

// Highlighted while a rule, not the default, chose the profile.
function indicatorActive(daemon) {
  return !!(daemon && daemon.connected && daemon.source && daemon.source !== "default")
}

function indicatorTooltip(daemon) {
  if (!daemon) return "Omalogi: daemon not running"
  if (daemon.error) return "Omalogi: " + daemon.error
  if (!daemon.connected || !daemon.active_profile) return "Omalogi: no mouse connected"
  var text = "Omalogi: profile " + daemon.active_profile
  if (daemon.source === "default") return text + " (default)"
  if (daemon.source) return text + " (" + daemon.source + (daemon.app ? " for " + daemon.app : "") + ")"
  return text
}

var TABS = ["buttons", "gshift", "sensitivity"]

// What an open payload asks for, or null: {"profile": 2, "tab": "gshift", "button": 3}.
// Unknown or malformed fields are ignored.
function openRequest(payload) {
  if (!payload || typeof payload !== "object") return null
  var profile = Number.isInteger(payload.profile) && payload.profile >= 1 ? payload.profile : null
  var tab = TABS.indexOf(payload.tab) !== -1 ? payload.tab : null
  var button = Number.isInteger(payload.button) && payload.button >= 0 ? payload.button : null
  if (profile === null && tab === null && button === null) return null
  return { profile: profile, tab: tab, button: button }
}

// ---- Editing ---------------------------------------------------------------
// A draft is a profile in the terms `omalogi profiles edit` accepts. Drafts are
// replaced, never mutated, so QML bindings see every change.

function copyDraft(draft) {
  return {
    number: draft.number,
    dpiStages: draft.dpiStages.slice(),
    defaultDpi: draft.defaultDpi,
    shiftDpi: draft.shiftDpi,
    rateHz: draft.rateHz,
    buttons: draft.buttons.slice(),
    gshift: draft.gshift.slice()
  }
}

function draftFromSlot(slot) {
  var p = slot.profile
  var stage = function(index) {
    var dpi = p.dpi_stages[index]
    return dpi === undefined ? null : dpi
  }
  return {
    number: slot.position + 1,
    dpiStages: p.dpi_stages.filter(function(dpi) { return dpi !== null }),
    defaultDpi: stage(p.default_dpi_index),
    shiftDpi: stage(p.shift_dpi_index),
    rateHz: p.report_rate_ms > 0 ? Math.round(1000 / p.report_rate_ms) : null,
    buttons: (slot.actions.buttons || []).slice(),
    gshift: (slot.actions.gshift_buttons || []).slice()
  }
}

// Changes one stage; the default and shift stages follow it when they pointed at it.
function setStage(draft, index, dpi) {
  var next = copyDraft(draft)
  var old = next.dpiStages[index]
  next.dpiStages[index] = dpi
  if (next.defaultDpi === old) next.defaultDpi = dpi
  if (next.shiftDpi === old) next.shiftDpi = dpi
  return next
}

function addStage(draft, dpi) {
  var next = copyDraft(draft)
  if (next.dpiStages.length < 5) next.dpiStages.push(dpi)
  return next
}

// Removes a stage; a default or shift stage that pointed at it must be chosen again.
function removeStage(draft, index) {
  var next = copyDraft(draft)
  var removed = next.dpiStages.splice(index, 1)[0]
  if (next.dpiStages.indexOf(removed) === -1) {
    if (next.defaultDpi === removed) next.defaultDpi = null
    if (next.shiftDpi === removed) next.shiftDpi = null
  }
  return next
}

function setField(draft, field, value) {
  var next = copyDraft(draft)
  next[field] = value
  return next
}

function setBinding(draft, table, slot, action) {
  var next = copyDraft(draft)
  next[table][slot] = action
  return next
}

// What still has to be chosen before the draft can be written, or "".
function draftProblem(draft) {
  if (draft.dpiStages.length === 0) return "Add at least one DPI stage."
  if (draft.defaultDpi === null || draft.dpiStages.indexOf(draft.defaultDpi) === -1) return "Choose the default DPI stage."
  if (draft.shiftDpi === null || draft.dpiStages.indexOf(draft.shiftDpi) === -1) return "Choose the DPI shift stage."
  var tables = [["buttons", "Button"], ["gshift", "G-Shift"]]
  for (var t = 0; t < tables.length; t++) {
    var slots = draft[tables[t][0]]
    for (var slot = 0; slot < slots.length; slot++) {
      if (slots[slot] === "key:") return "Type the keyboard shortcut for " + tables[t][1] + " slot " + slot + "."
    }
  }
  return ""
}

// The sensor's DPI range and step, from the list `omalogi info` reports.
function dpiBounds(info) {
  var values = (info && info.dpi_values) || []
  if (values.length === 0) return { min: 100, max: 25600, step: 50 }
  var step = values.length > 1 ? values[1] - values[0] : 50
  return { min: values[0], max: values[values.length - 1], step: step }
}

// A sensible DPI for a new stage: double the highest stage, within the sensor's range.
function nextStageDpi(draft, bounds) {
  var highest = draft.dpiStages.length > 0 ? Math.max.apply(null, draft.dpiStages) : bounds.min
  var doubled = Math.round((highest * 2) / bounds.step) * bounds.step
  return Math.max(bounds.min, Math.min(bounds.max, doubled))
}

// Slots that are physical buttons, plus slots the profile already binds (the wheel).
function editableSlots(original, table, buttonCount) {
  var slots = []
  original[table].forEach(function(action, slot) {
    if (slot < buttonCount || (action !== null && action !== "disabled")) slots.push(slot)
  })
  return slots
}

function isKeyAction(action) {
  return typeof action === "string" && action.indexOf("key:") === 0
}

function keyCombo(action) {
  return isKeyAction(action) ? action.slice(4) : ""
}

// The picker entry for a binding: key combos all live under "Keyboard shortcut…".
function actionChoice(action) {
  return isKeyAction(action) ? "key:" : (action || "")
}


// Arguments for `omalogi profiles edit`, covering only what differs from `original`.
function editArgs(draft, original, dryRun) {
  var args = ["profiles", "edit", String(draft.number)]
  var stagesChanged = draft.dpiStages.join(",") !== original.dpiStages.join(",")
  if (stagesChanged) args.push("--dpi", draft.dpiStages.join(","))
  if ((stagesChanged || draft.defaultDpi !== original.defaultDpi) && draft.defaultDpi !== null)
    args.push("--default-dpi", String(draft.defaultDpi))
  if ((stagesChanged || draft.shiftDpi !== original.shiftDpi) && draft.shiftDpi !== null)
    args.push("--shift-dpi", String(draft.shiftDpi))
  if (draft.rateHz !== original.rateHz && draft.rateHz !== null) args.push("--rate", String(draft.rateHz))
  var tables = [["buttons", "--button"], ["gshift", "--gshift"]]
  tables.forEach(function(table) {
    draft[table[0]].forEach(function(action, slot) {
      if (action !== null && action !== original[table[0]][slot]) args.push(table[1], slot + "=" + action)
    })
  })
  if (dryRun) args.push("--dry-run")
  return args
}


// ---- Shortcut recorder -----------------------------------------------------
// Linux evdev key codes (Qt's nativeScanCode minus 8 on Wayland) to the key names
// `omalogi` accepts after `key:`. Physical keys, as the mouse sends them, so the
// keyboard layout does not matter. A Rust test checks every name here parses.
var EVDEV_KEYS = {
  1: "esc", 2: "1", 3: "2", 4: "3", 5: "4", 6: "5", 7: "6", 8: "7", 9: "8", 10: "9", 11: "0",
  12: "minus", 13: "equal", 14: "backspace", 15: "tab",
  16: "q", 17: "w", 18: "e", 19: "r", 20: "t", 21: "y", 22: "u", 23: "i", 24: "o", 25: "p",
  26: "leftbracket", 27: "rightbracket", 28: "enter",
  30: "a", 31: "s", 32: "d", 33: "f", 34: "g", 35: "h", 36: "j", 37: "k", 38: "l",
  39: "semicolon", 40: "apostrophe", 41: "grave", 43: "backslash",
  44: "z", 45: "x", 46: "c", 47: "v", 48: "b", 49: "n", 50: "m",
  51: "comma", 52: "period", 53: "slash", 57: "space", 58: "capslock",
  59: "f1", 60: "f2", 61: "f3", 62: "f4", 63: "f5", 64: "f6", 65: "f7", 66: "f8", 67: "f9", 68: "f10",
  70: "scrolllock", 87: "f11", 88: "f12", 99: "printscreen",
  102: "home", 103: "up", 104: "pageup", 105: "left", 106: "right", 107: "end", 108: "down",
  109: "pagedown", 110: "insert", 111: "delete", 119: "pause",
  183: "f13", 184: "f14", 185: "f15", 186: "f16", 187: "f17", 188: "f18", 189: "f19", 190: "f20",
  191: "f21", 192: "f22", 193: "f23", 194: "f24"
}

// Left and right Ctrl, Shift, Alt and Super.
var EVDEV_MODIFIERS = [29, 42, 54, 56, 97, 100, 125, 126]

var QT_SHIFT = 0x02000000
var QT_CTRL = 0x04000000
var QT_ALT = 0x08000000
var QT_META = 0x10000000

// A key press as a recorded shortcut: `combo` like "ctrl+shift+t" for a usable key,
// `waiting` while only modifiers are held, `unsupported` for keys a mouse cannot send.
function recordKey(nativeScanCode, modifiers) {
  var code = nativeScanCode - 8
  if (EVDEV_MODIFIERS.indexOf(code) !== -1) return { combo: "", waiting: true, unsupported: false }
  var name = EVDEV_KEYS[code]
  if (name === undefined) return { combo: "", waiting: false, unsupported: true }
  var parts = []
  // The order omalogi prints them in, so a re-recorded shortcut compares equal.
  if (modifiers & QT_CTRL) parts.push("ctrl")
  if (modifiers & QT_SHIFT) parts.push("shift")
  if (modifiers & QT_ALT) parts.push("alt")
  if (modifiers & QT_META) parts.push("super")
  parts.push(name)
  return { combo: parts.join("+"), waiting: false, unsupported: false }
}

var KEY_LABELS = {
  ctrl: "Ctrl", shift: "Shift", alt: "Alt", super: "Super",
  esc: "Esc", minus: "-", equal: "=", backspace: "Backspace", tab: "Tab",
  leftbracket: "[", rightbracket: "]", enter: "Enter", semicolon: ";", apostrophe: "'",
  grave: "`", backslash: "\\", comma: ",", period: ".", slash: "/", space: "Space",
  capslock: "Caps Lock", scrolllock: "Scroll Lock", printscreen: "Print Screen", pause: "Pause",
  home: "Home", end: "End", pageup: "Page Up", pagedown: "Page Down", insert: "Insert",
  delete: "Delete", up: "Up", down: "Down", left: "Left", right: "Right"
}

// "ctrl+pageup" as the mouse's labels show it: "Ctrl+Page Up".
function comboLabel(combo) {
  return String(combo || "")
    .split("+")
    .filter(function(part) { return part !== "" })
    .map(function(part) { return KEY_LABELS[part] !== undefined ? KEY_LABELS[part] : part.toUpperCase() })
    .join("+")
}

// ---- Action picker ---------------------------------------------------------

var GROUP_ORDER = ["Mouse", "Keyboard", "Media", "DPI", "Profiles", "Scroll", "Other"]

// Catalog entries in display sections, filtered by `query` on label, value or group.
function actionSections(catalog, query) {
  var needle = String(query || "").trim().toLowerCase()
  var entries = catalog || []
  var groups = GROUP_ORDER.slice()
  entries.forEach(function(action) {
    if (groups.indexOf(action.group) === -1) groups.push(action.group)
  })
  var sections = []
  groups.forEach(function(group) {
    var actions = entries.filter(function(action) {
      if (action.group !== group) return false
      if (needle === "") return true
      return [action.label, action.value, action.group].some(function(text) {
        return String(text).toLowerCase().indexOf(needle) !== -1
      })
    })
    if (actions.length > 0) sections.push({ group: group, actions: actions })
  })
  return sections
}

// ---- Device canvas ---------------------------------------------------------

// Label cards beside the pictures, one per button, as G HUB and OpenLogi draw them.
// Views sit side by side at `height`, `viewGap` apart. Buttons in the left half of the
// first view get a card on the left, the rest on the right. Each side is ordered by
// height and cards stay as close to their button as they can without overlapping.
function calloutLayout(views, height, viewGap, cardHeight, cardGap) {
  var points = []
  var x = 0
  ;(views || []).forEach(function(view, index) {
    var width = viewWidth(view, height)
    ;(view.hotspots || []).forEach(function(hotspot) {
      points.push({
        slot: hotspot.slot,
        x: x + hotspot.x * width,
        y: hotspot.y * height,
        side: index === 0 && hotspot.x < 0.5 ? "left" : "right"
      })
    })
    x += width + viewGap
  })
  var step = cardHeight + cardGap
  var needed = height

  function place(side) {
    var cards = points
      .filter(function(point) { return point.side === side })
      .sort(function(a, b) { return a.y - b.y || a.slot - b.slot })
    if (cards.length * step - cardGap > height) {
      cards.forEach(function(card, i) { card.cardY = i * step })
      needed = Math.max(needed, cards.length * step - cardGap)
      return cards
    }
    cards.forEach(function(card, i) {
      var floor = i === 0 ? 0 : cards[i - 1].cardY + step
      card.cardY = Math.max(card.y - cardHeight / 2, floor)
    })
    for (var i = cards.length - 1; i >= 0; i--) {
      var ceiling = i === cards.length - 1 ? height - cardHeight : cards[i + 1].cardY - step
      cards[i].cardY = Math.max(0, Math.min(cards[i].cardY, ceiling))
    }
    return cards
  }

  var left = place("left")
  var right = place("right")
  return { picturesWidth: Math.max(0, x - viewGap), height: needed, left: left, right: right }
}

// ---- Unsaved changes -------------------------------------------------------

// Slots whose binding in `table` differs from the profile as read.
function changedSlots(draft, original, table) {
  var slots = []
  if (!draft || !original) return slots
  draft[table].forEach(function(action, slot) {
    if (action !== original[table][slot]) slots.push(slot)
  })
  return slots
}

function changeCount(draft, original) {
  if (!draft || !original) return 0
  var count = changedSlots(draft, original, "buttons").length + changedSlots(draft, original, "gshift").length
  if (draft.dpiStages.join(",") !== original.dpiStages.join(",")) count++
  if (draft.defaultDpi !== original.defaultDpi) count++
  if (draft.shiftDpi !== original.shiftDpi) count++
  if (draft.rateHz !== original.rateHz) count++
  return count
}

// Entries by slot number.
function indexBySlot(entries) {
  var index = {}
  ;(entries || []).forEach(function(entry) { index[entry.slot] = entry })
  return index
}

// Entries with no position on any picture view.
function entriesWithoutHotspot(entries, views) {
  var placed = {}
  ;(views || []).forEach(function(view) {
    ;(view.hotspots || []).forEach(function(hotspot) { placed[hotspot.slot] = true })
  })
  return (entries || []).filter(function(entry) { return !placed[entry.slot] })
}

// The tallest picture height, between `min` and `max`, at which the views fit `width`.
function fitPictureHeight(views, width, viewGap, min, max) {
  var aspect = 0
  ;(views || []).forEach(function(view) { aspect += view.width / view.height })
  if (aspect <= 0) return max
  var height = (width - viewGap * (views.length - 1)) / aspect
  return Math.floor(Math.max(min, Math.min(max, height)))
}

// Picker sections flattened for a list: each section's header row, then its actions.
function actionRows(sections) {
  var rows = []
  ;(sections || []).forEach(function(section) {
    rows.push({ kind: "header", group: section.group })
    section.actions.forEach(function(action) {
      rows.push({ kind: "action", value: action.value, label: action.label, group: section.group })
    })
  })
  return rows
}

// Held modifiers as the recorder shows them: "Ctrl+Shift".
function modifiersLabel(modifiers) {
  var parts = []
  if (modifiers & QT_CTRL) parts.push("Ctrl")
  if (modifiers & QT_SHIFT) parts.push("Shift")
  if (modifiers & QT_ALT) parts.push("Alt")
  if (modifiers & QT_META) parts.push("Super")
  return parts.join("+")
}

// A button's name: G HUB's G-number where the picture's positions are verified.
function buttonName(slot, verified) {
  return verified ? "G" + (slot + 1) : "Slot " + slot
}

// How an action reads: the mouse's own label while unchanged, otherwise the catalog
// label, the shortcut, or the action text itself.
function actionLabel(catalog, action, deviceLabel, changed) {
  if (!changed && deviceLabel) return deviceLabel
  if (action === null || action === undefined) return deviceLabel || "Unsupported binding"
  if (isKeyAction(action)) return comboLabel(keyCombo(action))
  var matches = (catalog || []).filter(function(entry) { return entry.value === action })
  return matches.length > 0 ? matches[0].label : action
}

// Canvas and inspector entries for one table ("buttons" or "gshift") of a profile.
function slotEntries(slot, draft, original, catalog, table, buttonCount, verified) {
  if (!slot || !draft || !original) return []
  var labels = (table === "gshift" ? slot.labels.gshift_buttons : slot.labels.buttons) || []
  return editableSlots(original, table, buttonCount).map(function(number) {
    var action = draft[table][number]
    var changed = action !== original[table][number]
    return {
      slot: number,
      // Slots past the physical buttons (extra wheel bindings) have no G-number.
      name: buttonName(number, verified && number < buttonCount),
      label: actionLabel(catalog, action, labels[number], changed),
      changed: changed,
      action: action
    }
  })
}

// Picture views from `omalogi picture`, or [] when there is no usable picture.
function pictureViews(picture) {
  if (!picture || !Array.isArray(picture.views)) return []
  return picture.views.filter(function(view) {
    return view && typeof view.image === "string" && view.width > 0 && view.height > 0
  })
}

// Width of a view drawn at `height`, keeping its aspect ratio.
function viewWidth(view, height) {
  return Math.round((height * view.width) / view.height)
}

