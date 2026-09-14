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

// DPI stages in order, skipping unused ones, marking the default and shift stages.
function dpiStages(profile) {
  var stages = []
  profile.dpi_stages.forEach(function(dpi, index) {
    if (dpi === null) return
    stages.push({
      dpi: dpi,
      isDefault: index === profile.default_dpi_index,
      isShift: index === profile.shift_dpi_index
    })
  })
  return stages
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
  return ""
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

function hasChanges(draft, original) {
  return editArgs(draft, original, false).length > 3
}

// Bound slots as {slot, label}. omalogi sends null for unbound slots.
function boundSlots(labels) {
  var slots = []
  ;(labels || []).forEach(function(label, slot) {
    if (label !== null) slots.push({ slot: slot, label: label })
  })
  return slots
}
