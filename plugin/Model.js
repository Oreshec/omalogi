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

// Bound slots as {slot, label}. omalogi sends null for unbound slots.
function boundSlots(labels) {
  var slots = []
  ;(labels || []).forEach(function(label, slot) {
    if (label !== null) slots.push({ slot: slot, label: label })
  })
  return slots
}
