import QtQuick
import qs.Commons

// Omalogi's mark: a wired mouse on a 16 × 16 grid, drawn in whole pixels. While `busy`,
// the scroll wheel rolls and the cable sways, as in the README logo
// (scripts/pixel-mark.py draws the same sprite).
Canvas {
  id: mark

  property color ink: Color.menu.text
  property color accent: Color.accent
  property bool busy: false
  // Advances while busy; each step is one wheel ridge.
  property int frame: 0

  implicitWidth: 32
  implicitHeight: 32

  // k outline, s scroll wheel. The top cable pixel sways, so row 0 is drawn separately.
  readonly property var sprite: [
    "................",
    ".......k........",
    ".......k........",
    ".....kkkkk......",
    "....k..k..k.....",
    "...k...s...k....",
    "...k...s...k....",
    "...k...k...k....",
    "...kkkkkkkkk....",
    "...k.......k....",
    "...k.......k....",
    "...k.......k....",
    "...k.......k....",
    "....k.....k.....",
    ".....kkkkk......",
    "................"
  ]

  function css(color) {
    return "rgb(" + Math.round(color.r * 255) + "," + Math.round(color.g * 255) + "," + Math.round(color.b * 255) + ")"
  }

  Timer {
    interval: 130
    repeat: true
    running: mark.busy && mark.visible
    onTriggered: mark.frame++
    onRunningChanged: if (!running) mark.frame = 0
  }

  onFrameChanged: requestPaint()
  onInkChanged: requestPaint()
  onAccentChanged: requestPaint()
  onWidthChanged: requestPaint()
  onHeightChanged: requestPaint()

  onPaint: {
    var ctx = getContext("2d")
    ctx.reset()
    // Whole pixels only: the grid never scales to a fraction.
    var cell = Math.max(1, Math.floor(Math.min(width, height) / 16))
    var ox = Math.floor((width - 16 * cell) / 2)
    var oy = Math.floor((height - 16 * cell) / 2)
    var ridge = frame % 2
    var sway = Math.floor(frame / 7) % 2
    var ink = css(mark.ink)
    var accent = css(mark.accent)
    function pixel(x, y, color, alpha) {
      ctx.globalAlpha = alpha
      ctx.fillStyle = color
      ctx.fillRect(ox + x * cell, oy + y * cell, cell, cell)
    }
    for (var y = 1; y < 16; y++) {
      for (var x = 0; x < 16; x++) {
        var ch = sprite[y].charAt(x)
        if (ch === "k") pixel(x, y, ink, 1)
        else if (ch === "s") pixel(x, y, accent, (y + ridge) % 2 === 1 ? 1 : 0.35)
      }
    }
    pixel(sway ? 6 : 8, 0, ink, 1)
    ctx.globalAlpha = 1
  }
}
