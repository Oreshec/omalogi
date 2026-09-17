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

// Omalogi installs in two parts: this plugin, added like any Omarchy plugin, and its
// helper (the omalogi command and a udev rule), from the installer in this checkout.
var PLUGIN_ADD_COMMAND = "omarchy plugin add https://github.com/elberacasa/omalogi --enable"

// `script` quoted for a shell, so a plugin folder with spaces or quotes stays one word.
function shellQuote(script) {
  return "'" + String(script).replace(/'/g, "'\\''") + "'"
}

// The command that runs the plugin's own install.sh: the reviewed code in this checkout,
// never a download piped to a shell.
function helperInstallCommand(installScript) {
  return "bash " + shellQuote(installScript)
}

// Runs it in Omarchy's floating terminal, as the bar's update button runs omarchy-update.
function helperInstallArgv(installScript) {
  return ["omarchy-launch-floating-terminal-with-presentation", helperInstallCommand(installScript)]
}

// A local path from a file: URL, e.g. Qt.resolvedUrl("../install.sh").
function localPath(url) {
  return decodeURIComponent(String(url).replace(/^file:\/\//, ""))
}

// The oldest `omalogi serve` request format this plugin works with.
var REQUIRED_PROTOCOL = 3

// What stops Omalogi reaching the mouse, from a failed load's message: "helper" (not
// installed), "access" (no permission), "device" (no mouse), or "error".
function setupState(message, kind) {
  if (kind === "directory_checksum") return "directory"
  var text = String(message || "")
  if (text === "") return ""
  // Helpers before protocol 3 refuse a damaged profile directory with no way to repair it.
  if (text.indexOf("profile directory has an invalid checksum") !== -1) return "outdated"
  if (text.indexOf("not installed or not on PATH") !== -1) return "helper"
  if (text.indexOf("permission denied opening") !== -1) return "access"
  // Helpers from 0.2 say "mouse"; older ones said "device".
  if (text.indexOf("no supported Logitech mouse found") !== -1) return "device"
  if (text.indexOf("no supported Logitech device found") !== -1) return "device"
  return "error"
}

// The connected mouse's support, from a `state` answer. Helpers before protocol 2 only
// opened the verified G502 X, so no report means that.
function support(state) {
  if (state && state.support) return state.support
  return { name: "G502 X", verified: true, editable: true, accepted: true }
}

// The header badge for a mouse that is not verified, or null.
function supportBadge(support) {
  if (!support || support.verified) return null
  if (!support.editable) {
    return {
      text: "Not supported yet",
      action: "Report it",
      detail: "Omalogi cannot read this mouse's profile layout (format " + support.profile_format + ") yet."
    }
  }
  return {
    text: "Untested",
    action: "Help verify it",
    detail: "Omalogi has not been tested on the " + support.name + " yet. Every change is backed up and verified."
  }
}

// Whether saving must first ask the user to accept editing an untested mouse.
function needsAcceptance(support) {
  return !!support && !support.verified && !!support.editable && !support.accepted
}

function hex4(value) {
  return ("0000" + Number(value || 0).toString(16)).slice(-4)
}

// The Mouse report form, prefilled with the model, its product id and active firmware
// version only: never the unit ID, serials or backups.
function reportUrl(info, support) {
  var name = support && support.name ? support.name : (info && info.name) || "Logitech mouse"
  var fields = {
    template: "mouse.yml",
    title: "[Mouse]: " + name,
    model: "Logitech " + name,
    device_id: hex4(info && info.vendor_id) + ":" + hex4(info && info.product_id)
  }
  var firmware = info ? activeFirmware(info) : ""
  if (firmware !== "") fields.firmware = firmware
  return "https://github.com/elberacasa/omalogi/issues/new?" + Object.keys(fields).map(function(key) {
    return key + "=" + encodeURIComponent(fields[key])
  }).join("&")
}

// Whether the helper is older than this plugin needs. Helpers before the protocol was
// reported speak protocol 1.
function helperOutdated(state) {
  var protocol = state && state.helper && state.helper.protocol ? state.helper.protocol : 1
  return protocol < REQUIRED_PROTOCOL
}

// The setup screen for a state: {title, body, action}. No action means only trying again helps.
function setupCopy(kind, message) {
  switch (kind) {
  case "helper":
    return {
      title: "Install Omalogi's helper",
      body: "To talk to your mouse, the plugin needs its helper: the omalogi command and a udev rule that lets your user reach the mouse. The installer checks its download, asks for your password once, and asks before adding the bar indicator and automatic profile switching.",
      action: "Install helper",
      installer: true
    }
  case "access":
    return {
      title: "Allow access to your mouse",
      body: "The helper is installed but cannot open the mouse. The installer adds Omalogi's udev rule, which gives your login session access to supported Logitech mice and their receivers, and nothing else.",
      action: "Allow access",
      installer: true
    }
  case "outdated":
    return {
      title: "Update Omalogi's helper",
      body: "This version of the plugin needs a newer helper. The installer updates it in place.",
      action: "Update helper",
      installer: true
    }
  case "directory":
    return {
      title: "Repair the profile list on your mouse",
      body: "The list of profiles stored on your mouse fails its checksum, so Omalogi is not changing anything. Repair first checks that every profile it lists is intact, backs up the mouse's memory, then rewrites only that list and verifies it.",
      action: "Repair",
      installer: false
    }
  case "device":
    return {
      title: "Plug in your mouse",
      body: "Connect a Logitech G-series mouse over USB, or turn on a wireless one paired to its receiver, then try again.",
      action: "",
      installer: false
    }
  default:
    return { title: "Omalogi could not read your mouse", body: String(message || ""), action: "", installer: false }
  }
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

// Whether the mouse is using the profile, which decides when its changes are felt.
function profileStatus(slot) {
  if (!slot.enabled) return "Turned off on the mouse  ·  changes apply once it is turned on and activated"
  if (slot.active) return "In use  ·  " + reportRate(slot.profile)
  return "Not in use  ·  " + reportRate(slot.profile) + "  ·  changes apply when you activate it"
}

// Why a profile cannot be activated, or "" when it can.
function activationRefusal(slot) {
  var number = slot.position + 1
  if (!slot.enabled) return "Profile " + number + " is turned off on the mouse, so it can't be activated."
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

// The G-numbers of the physical buttons that hold G-Shift on the default layer, e.g.
// ["G5"]. Without one, the G-Shift layer cannot be reached on the mouse.
function gshiftButtons(draft, buttonCount) {
  if (!draft) return []
  var names = []
  ;(draft.buttons || []).forEach(function(action, slot) {
    if (action === "gshift" && slot < buttonCount) names.push(buttonName(slot, true))
  })
  return names
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

// ---- Icons -----------------------------------------------------------------
// Icon names from Icons.js for actions and their groups.

var ACTION_ICONS = {
  left: "mouse-left",
  right: "mouse-right",
  middle: "mouse",
  back: "circle-arrow-left",
  forward: "circle-arrow-right",
  "dpi-up": "gauge",
  "dpi-down": "gauge",
  "dpi-cycle": "gauge",
  "dpi-default": "gauge",
  "dpi-shift": "gauge",
  gshift: "layers",
  "profile-next": "square-arrow-right",
  "profile-previous": "square-arrow-left",
  "profile-cycle": "refresh-cw",
  "scroll-left": "chevrons-left",
  "scroll-right": "chevrons-right",
  "scroll-up": "chevrons-up",
  "scroll-down": "chevrons-down",
  "media:play-pause": "play",
  "media:next-track": "skip-forward",
  "media:previous-track": "skip-back",
  "media:volume-up": "volume-2",
  "media:volume-down": "volume-1",
  "media:mute": "volume-x",
  disabled: "ban"
}

var GROUP_ICONS = {
  Mouse: "mouse",
  Keyboard: "keyboard",
  Media: "play",
  DPI: "gauge",
  Profiles: "layers",
  Scroll: "move",
  Other: "settings"
}

function actionIcon(action) {
  if (isKeyAction(action)) return "keyboard"
  if (typeof action === "string" && action.indexOf("button:") === 0) return "mouse"
  return ACTION_ICONS[action] || "mouse-pointer-click"
}

function groupIcon(group) {
  return GROUP_ICONS[group] || "layout-grid"
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

// Names of the buttons whose binding is `action` (keyboard shortcuts count as one action).
function assignedNames(entries, action) {
  return (entries || [])
    .filter(function(entry) { return actionChoice(entry.action) === action })
    .map(function(entry) { return entry.name })
}

// The catalog's groups in display order.
function actionGroups(catalog) {
  return actionSections(catalog, "").map(function(section) { return section.group })
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

// The index of the picture view that shows `slot`, or -1.
function viewForSlot(views, slot) {
  var found = -1
  ;(views || []).forEach(function(view, index) {
    if (found !== -1) return
    if ((view.hotspots || []).some(function(hotspot) { return hotspot.slot === slot })) found = index
  })
  return found
}

// What "use default" puts back for `slot` in `table`: the mouse's factory binding, or
// null when the factory profile is unknown.
function factoryAction(onboard, table, slot) {
  var factory = onboard && onboard.factory
  if (!factory) return null
  var list = table === "gshift" ? factory.gshift_buttons : factory.buttons
  var action = list ? list[slot] : undefined
  return action === undefined ? null : action
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

// ---- Applying --------------------------------------------------------------

// The DPI slider is logarithmic, so 400 to 1600 gets as much travel as 6400 to 25600.
var SLIDER_STEPS = 1000

function dpiToPosition(dpi, bounds) {
  if (dpi <= bounds.min) return 0
  if (dpi >= bounds.max) return SLIDER_STEPS
  return Math.round((SLIDER_STEPS * Math.log(dpi / bounds.min)) / Math.log(bounds.max / bounds.min))
}

function positionToDpi(position, bounds) {
  var raw = bounds.min * Math.pow(bounds.max / bounds.min, position / SLIDER_STEPS)
  var snapped = Math.round(raw / bounds.step) * bounds.step
  return Math.max(bounds.min, Math.min(bounds.max, snapped))
}

// ---- DPI bar ---------------------------------------------------------------
// DPI levels on one logarithmic bar, as G HUB draws them. Levels are kept from low to
// high, the order the DPI buttons step through.

function dpiFraction(dpi, bounds) {
  return dpiToPosition(dpi, bounds) / SLIDER_STEPS
}

function dpiAtFraction(fraction, bounds) {
  return positionToDpi(Math.max(0, Math.min(1, fraction)) * SLIDER_STEPS, bounds)
}

// One node per level: {index, dpi, fraction, isDefault, isShift}.
function dpiNodes(draft, bounds) {
  return draft.dpiStages.map(function(dpi, index) {
    return {
      index: index,
      dpi: dpi,
      fraction: dpiFraction(dpi, bounds),
      isDefault: dpi === draft.defaultDpi,
      isShift: dpi === draft.shiftDpi
    }
  })
}

// Tick marks: doublings from the sensor's lowest DPI, and its highest.
function dpiTicks(bounds) {
  var ticks = []
  for (var dpi = bounds.min; dpi <= bounds.max; dpi *= 2) ticks.push(dpi)
  if (ticks[ticks.length - 1] !== bounds.max) ticks.push(bounds.max)
  return ticks.map(function(value) { return { dpi: value, fraction: dpiFraction(value, bounds) } })
}

function sortStages(draft) {
  var next = copyDraft(draft)
  next.dpiStages.sort(function(a, b) { return a - b })
  return next
}

// Adds a level where it belongs in the order; at most five.
function insertStage(draft, dpi) {
  if (draft.dpiStages.length >= 5) return draft
  return sortStages(addStage(draft, dpi))
}

// Removes a level. When it was the default or DPI shift level, the nearest remaining
// level takes that role, so a profile is never left without one.
function removeStageKeepingRoles(draft, index) {
  var removed = draft.dpiStages[index]
  var next = removeStage(draft, index)
  if (next.dpiStages.length === 0) return next
  var nearest = next.dpiStages.reduce(function(best, dpi) {
    return Math.abs(dpi - removed) < Math.abs(best - removed) ? dpi : best
  })
  if (next.defaultDpi === null) next.defaultDpi = nearest
  if (next.shiftDpi === null) next.shiftDpi = nearest
  return next
}

// ---- Saving through `omalogi serve` ----------------------------------------

// The fields `omalogi serve` applies: only what differs from `original`. Stage edits
// carry the default and shift stages, as the stages may have moved.
function serveChanges(draft, original) {
  var changes = {}
  var stagesChanged = draft.dpiStages.join(",") !== original.dpiStages.join(",")
  if (stagesChanged) changes.dpi = draft.dpiStages.slice()
  if ((stagesChanged || draft.defaultDpi !== original.defaultDpi) && draft.defaultDpi !== null)
    changes.default_dpi = draft.defaultDpi
  if ((stagesChanged || draft.shiftDpi !== original.shiftDpi) && draft.shiftDpi !== null)
    changes.shift_dpi = draft.shiftDpi
  if (draft.rateHz !== original.rateHz && draft.rateHz !== null) changes.rate = draft.rateHz
  ;[["buttons", "buttons"], ["gshift", "gshift"]].forEach(function(pair) {
    var table = {}
    var any = false
    draft[pair[0]].forEach(function(action, slot) {
      if (action !== null && action !== original[pair[0]][slot]) {
        table[String(slot)] = action
        any = true
      }
    })
    if (any) changes[pair[1]] = table
  })
  return changes
}

// The footer after a save (`undo` false) or an undo reply.
function saveStatus(result, undo) {
  var number = result.slot.position + 1
  var lead = undo ? "Undid the last change to profile " + number : "Saved to profile " + number
  var effect = result.takes_effect
  if (!effect) return { text: "Profile " + number + " already has these settings.", isError: false }
  if (effect.state === "now") return { text: lead + ". The mouse is using it now.", isError: false }
  if (effect.state === "when_activated") {
    return { text: lead + ". It applies when you activate this profile.", isError: false }
  }
  return {
    text: lead + ", but the mouse has not loaded it: " + (effect.reason || "unknown reason") + ".",
    isError: true
  }
}

// `onboard` with one profile replaced by a fresh read of it.
function withSlot(onboard, slot) {
  return {
    mode: onboard.mode,
    description: onboard.description,
    active_position: slot.active ? slot.position : onboard.active_position,
    profiles: onboard.profiles.map(function(existing) {
      return existing.position === slot.position ? slot : existing
    })
  }
}

// `onboard` with the profile at `position` in use.
function withActive(onboard, position) {
  return {
    mode: onboard.mode,
    description: onboard.description,
    active_position: position,
    profiles: onboard.profiles.map(function(slot) {
      var copy = Object.assign({}, slot)
      copy.active = slot.position === position
      return copy
    })
  }
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

