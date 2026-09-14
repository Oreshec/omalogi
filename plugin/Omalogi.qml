import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Omalogi's overlay: the connected mouse's onboard profiles, edited the way G HUB does it.
// Changes save themselves a moment after the last edit, through one long-lived
// `omalogi serve` that keeps the mouse open. Every write is backed up and verified, and
// Undo puts back what a write replaced.
Item {
  id: root

  // Injected by the shell.
  property var shell: null
  property var manifest: null

  property bool opened: false
  property bool mounted: false
  property var info: null
  property var onboard: null
  // The daemon's published state, or null when it is not running.
  property var daemon: null
  property var catalog: []
  // `omalogi picture`: the mouse's picture with button positions, or null.
  property var picture: null
  property string loadError: ""
  property string notice: ""
  property bool noticeIsError: false
  property int cursor: 0

  // "buttons", "gshift" or "sensitivity".
  property string tab: "buttons"
  // The selected profile as the mouse has it, and with the edits not saved yet.
  property var original: null
  property var draft: null
  property int selectedSlot: -1
  property int hoveredSlot: -1
  // What the open payload asked for, applied once profiles have loaded.
  property var pendingOpen: null

  property bool loading: false
  // At most one write is in flight; edits made meanwhile are saved after it.
  property bool saving: false
  property bool undoing: false
  // Saved writes this session that Undo can put back.
  property int undoDepth: 0
  // A profile to select once the write in flight is done.
  property int pendingCursor: -1
  // Changes the mouse turned out to have already, so they are not sent again.
  property string unchangedChanges: ""

  readonly property var profiles: root.onboard ? root.onboard.profiles : []
  readonly property var selected: root.profiles.length > 0
    ? root.profiles[Model.clampCursor(root.cursor, root.profiles.length)]
    : null
  readonly property bool ready: root.info !== null && root.onboard !== null
  readonly property int changes: Model.changeCount(root.draft, root.original)
  readonly property bool dirty: root.changes > 0
  readonly property string problem: root.draft ? Model.draftProblem(root.draft) : ""
  readonly property var views: Model.pictureViews(root.picture)
  readonly property bool slotsVerified: root.picture !== null && root.picture.slots_verified === true
  readonly property int buttonCount: root.onboard ? root.onboard.description.button_count : 0
  readonly property string table: root.tab === "gshift" ? "gshift" : "buttons"
  readonly property var entries: Model.slotEntries(
    root.selected, root.draft, root.original, root.catalog, root.table, root.buttonCount, root.slotsVerified)
  readonly property var selectedEntry: {
    var entry = Model.indexBySlot(root.entries)[root.selectedSlot]
    return entry === undefined ? null : entry
  }
  readonly property bool anyDisabled: root.profiles.some(function(slot) { return !slot.enabled })

  readonly property int cardWidth: Math.min(Style.space(1400), panel.width - Style.gapsOut * 2)
  readonly property int cardHeight: Math.min(Style.space(820), panel.height - Style.gapsOut * 2)
  readonly property int headerHeight: Math.max(Style.space(40), Style.font.heading + Style.spacing.controlPaddingY * 2)
  readonly property int railWidth: Style.space(196)
  readonly property int inspectorWidth: Style.space(340)

  function open(payloadJson) {
    exitAnimation.stop()
    root.mounted = true
    root.opened = true
    enterAnimation.restart()
    root.pendingOpen = Model.openRequest(Model.parseJson(payloadJson))
    if (root.ready) root.applyOpenRequest()
    root.refresh()
    if (root.catalog.length === 0 && !catalogCommand.running) catalogCommand.start(["actions", "--json"])
    if (root.picture === null && !pictureCommand.running) pictureCommand.start(["picture", "--json"])
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    if (!root.mounted || !root.opened) return
    // Edits waiting for the timer are saved now; the save finishes after the overlay closes.
    root.saveNow()
    root.opened = false
    enterAnimation.stop()
    exitAnimation.restart()
  }

  function finishClose() {
    root.mounted = false
    root.selectedSlot = -1
    root.stopServerWhenIdle()
    if (root.shell && root.manifest) root.shell.hide(root.manifest.id)
  }

  // The server keeps the mouse open; it stops once the overlay is closed and idle.
  function stopServerWhenIdle() {
    if (!root.opened && server.inFlight === 0 && !saveTimer.running) server.stop()
  }

  function refresh() {
    if (root.loading) return
    root.loading = true
    root.loadError = ""
    server.request({ cmd: "state" }, function(ok, result) {
      root.loading = false
      if (!ok) {
        root.failLoad(result)
        return
      }
      var first = root.onboard === null
      root.info = result.info
      root.onboard = result.onboard
      if (first) root.cursor = Model.initialCursor(result.onboard)
      if (first || (!root.dirty && !root.saving && !root.undoing)) root.loadDraft()
      Qt.callLater(root.applyOpenRequest)
      root.stopServerWhenIdle()
    })
  }

  function retry() {
    server.stop()
    root.loadError = ""
    root.refresh()
  }

  function applyOpenRequest() {
    var request = root.pendingOpen
    if (request === null || !root.ready) return
    root.pendingOpen = null
    if (request.profile !== null) root.selectProfile(request.profile - 1)
    if (request.tab !== null) root.tab = request.tab
    if (request.button !== null) root.selectedSlot = request.button
  }

  function loadDraft() {
    root.original = root.selected ? Model.draftFromSlot(root.selected) : null
    root.draft = root.original
    root.unchangedChanges = ""
  }

  // Selects a profile, saving the current one's edits first.
  function selectProfile(index) {
    var next = Model.clampCursor(index, root.profiles.length)
    if (next === root.cursor && root.draft !== null) return
    saveTimer.stop()
    if (root.saving || root.undoing || root.save()) {
      root.pendingCursor = next
      return
    }
    if (root.dirty && root.problem !== "") {
      root.say("Changes to profile " + root.draft.number + " were not saved: " + root.problem, true)
    }
    root.cursor = next
    root.loadDraft()
  }

  // `immediate` edits (a pick, a click, a released slider) save almost at once; typed
  // values wait for a pause so each keystroke is not a write.
  function updateDraft(next, immediate) {
    root.draft = next
    root.say("", false)
    if (!root.dirty) {
      saveTimer.stop()
      return
    }
    saveTimer.interval = immediate ? 120 : 700
    saveTimer.restart()
  }

  function choose(action) {
    if (!root.draft || root.selectedSlot < 0) return
    root.updateDraft(Model.setBinding(root.draft, root.table, root.selectedSlot, action), true)
  }

  function revertSlot() {
    if (!root.draft || root.selectedSlot < 0) return
    var saved = root.original[root.table][root.selectedSlot]
    root.updateDraft(Model.setBinding(root.draft, root.table, root.selectedSlot, saved), true)
  }

  function saveNow() {
    if (!saveTimer.running) return
    saveTimer.stop()
    root.save()
  }

  // Sends the unsaved edits. Returns true when a write was started or is already coming.
  function save() {
    if (!root.draft || !root.original || !root.dirty) return false
    if (root.problem !== "") {
      root.say(root.problem, true)
      return false
    }
    if (root.saving || root.undoing) {
      saveTimer.restart()
      return true
    }
    var changes = Model.serveChanges(root.draft, root.original)
    var key = root.draft.number + ":" + JSON.stringify(changes)
    if (key === root.unchangedChanges) return false
    root.saving = true
    server.request(Object.assign({ cmd: "apply", profile: root.draft.number }, changes), function(ok, result) {
      root.saving = false
      if (ok) {
        root.onboard = Model.withSlot(root.onboard, result.slot)
        root.undoDepth = result.undo
        if (result.takes_effect === null) root.unchangedChanges = key
        // The edits made while saving stay in the draft and save next.
        if (root.draft && root.draft.number === result.slot.position + 1) {
          root.original = Model.draftFromSlot(result.slot)
        }
        var status = Model.saveStatus(result, false)
        root.say(status.text, status.isError)
      } else {
        root.say("Not saved: " + result, true)
      }
      root.afterWrite(ok)
    })
    return true
  }

  function undo() {
    if (root.saving || root.undoing) return
    saveTimer.stop()
    // The latest change is one not saved yet: drop it without touching the mouse.
    if (root.dirty) {
      root.draft = root.original
      root.say("Undid the changes that were not saved yet.", false)
      return
    }
    if (root.undoDepth === 0) return
    root.undoing = true
    server.request({ cmd: "undo" }, function(ok, result) {
      root.undoing = false
      if (ok) {
        root.onboard = Model.withSlot(root.onboard, result.slot)
        root.undoDepth = result.undo
        root.unchangedChanges = ""
        if (root.draft && root.draft.number === result.slot.position + 1) {
          root.original = Model.draftFromSlot(result.slot)
          root.draft = root.original
        }
        var status = Model.saveStatus(result, true)
        root.say(status.text, status.isError)
      } else {
        root.say("Undo failed: " + result, true)
      }
      root.afterWrite(ok)
    })
  }

  function afterWrite(ok) {
    var more = ok && root.dirty && root.problem === ""
    if (more && root.opened && root.pendingCursor < 0) {
      saveTimer.restart()
      return
    }
    if (more && root.save()) return
    if (root.pendingCursor >= 0) {
      var next = root.pendingCursor
      root.pendingCursor = -1
      root.cursor = next
      root.loadDraft()
    }
    root.stopServerWhenIdle()
  }

  function activate() {
    var slot = root.selected
    if (!slot) return
    var refusal = Model.activationRefusal(slot)
    if (refusal !== "") {
      root.say(refusal, false)
      return
    }
    var position = slot.position
    server.request({ cmd: "activate", profile: position + 1 }, function(ok, result) {
      if (!ok) {
        root.say(result, true)
        return
      }
      root.onboard = Model.withActive(root.onboard, position)
      root.say("Profile " + (position + 1) + " is now in use.", false)
    })
  }

  function hoverSlot(slot, hovered) {
    if (hovered) root.hoveredSlot = slot
    else if (root.hoveredSlot === slot) root.hoveredSlot = -1
  }

  function say(message, isError) {
    root.notice = message
    root.noticeIsError = isError
  }

  // A failed load replaces the view until data exists; afterwards it only shows in the footer.
  function failLoad(message) {
    if (root.ready) root.say(message, true)
    else root.loadError = message
  }

  function daemonUpdated(state) {
    var previous = root.daemon
    root.daemon = state
    // The daemon switched profiles: its state names the new one, so no read is needed.
    var switched = state !== null && state.active_profile !== null
      && (previous === null || previous.active_profile !== state.active_profile)
    if (root.ready && switched) root.onboard = Model.withActive(root.onboard, state.active_profile - 1)
  }

  OmalogiServer {
    id: server
    onFailed: function(message) {
      root.loading = false
      root.saving = false
      root.undoing = false
      root.failLoad(message)
    }
  }

  Timer {
    id: saveTimer
    interval: 700
    onTriggered: root.save()
  }

  // Needs no device, so it runs as its own command.
  OmalogiCommand {
    id: catalogCommand
    onFinished: function(exitCode, stdout, stderr) {
      var parsed = exitCode === 0 ? Model.parseJson(stdout) : null
      if (parsed === null) root.say(Model.errorMessage(stderr, exitCode), true)
      else root.catalog = parsed
    }
  }

  // Reads sysfs and the cache, never the device. Without a picture the buttons are
  // shown as cards alone.
  OmalogiCommand {
    id: pictureCommand
    onFinished: function(exitCode, stdout, stderr) {
      root.picture = exitCode === 0 ? Model.parseJson(stdout) : null
    }
  }

  FileView {
    path: Quickshell.env("XDG_RUNTIME_DIR") + "/omalogi/state.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.daemonUpdated(Model.parseJson(text()))
    // text() is stale inside the change signal, so re-read and parse in onLoaded.
    onFileChanged: reload()
    onLoadFailed: root.daemonUpdated(null)
  }

  ParallelAnimation {
    id: enterAnimation
    NumberAnimation { target: card; property: "opacity"; from: 0; to: 1; duration: 180; easing.type: Easing.OutCubic }
    NumberAnimation { target: rise; property: "y"; from: Style.space(10); to: 0; duration: 240; easing.type: Easing.OutCubic }
  }

  SequentialAnimation {
    id: exitAnimation
    ParallelAnimation {
      NumberAnimation { target: card; property: "opacity"; to: 0; duration: 120; easing.type: Easing.InCubic }
      NumberAnimation { target: rise; property: "y"; to: Style.space(6); duration: 120; easing.type: Easing.InCubic }
    }
    ScriptAction { script: root.finishClose() }
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.body
  }

  PanelWindow {
    id: panel
    visible: root.mounted
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "omalogi"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
    exclusionMode: ExclusionMode.Ignore

    Rectangle {
      anchors.fill: parent
      color: Color.menu.scrim
      opacity: card.opacity
    }

    MouseArea {
      anchors.fill: parent
      onClicked: root.close()
    }

    BorderSurface {
      id: card
      width: root.cardWidth
      height: root.cardHeight
      anchors.centerIn: parent
      radius: Style.cornerRadius
      color: Color.menu.background
      borderSpec: Border.surfaceSpec("menu", "border", Color.menu.border, Math.max(1, Style.space(2)))
      padding: Style.spacing.panelPadding
      opacity: 0

      transform: Translate { id: rise }

      MouseArea { anchors.fill: parent; onClicked: keyCatcher.forceActiveFocus() }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.onPressed: function(event) {
          var ctrl = (event.modifiers & Qt.ControlModifier) !== 0
          if (event.key === Qt.Key_Escape) {
            if (root.selectedSlot >= 0) root.selectedSlot = -1
            else root.close()
          } else if (ctrl && event.key === Qt.Key_Z) {
            root.undo()
          } else if (ctrl && event.key === Qt.Key_S) {
            saveTimer.stop()
            root.save()
          } else if (event.key === Qt.Key_Down || event.text === "j") {
            root.selectProfile(root.cursor + 1)
          } else if (event.key === Qt.Key_Up || event.text === "k") {
            root.selectProfile(root.cursor - 1)
          } else if (event.text === "1") {
            root.tab = "buttons"
          } else if (event.text === "2") {
            root.tab = "gshift"
          } else if (event.text === "3") {
            root.tab = "sensitivity"
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            root.activate()
          } else if (event.text === "r" && !root.dirty && !root.saving) {
            root.refresh()
          } else {
            return
          }
          event.accepted = true
        }
      }

      Item {
        anchors.fill: parent
        anchors.topMargin: card.contentTopInset
        anchors.rightMargin: card.contentRightInset
        anchors.bottomMargin: card.contentBottomInset
        anchors.leftMargin: card.contentLeftInset

        // ---- Header -----------------------------------------------------
        Item {
          id: header
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          height: root.headerHeight

          Label {
            id: brand
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            text: "Omalogi"
            font.pixelSize: Style.font.heading
            font.bold: true
          }

          Label {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, parent.width - brand.width - Style.spacing.panelGap)
            horizontalAlignment: Text.AlignRight
            opacity: 0.6
            text: root.info ? Model.deviceSummary(root.info) : ""
          }
        }

        // ---- Footer: save status and Undo -------------------------------
        Item {
          id: footer
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.bottom: parent.bottom
          height: Style.spacing.controlHeight + Style.spacing.sm * 2

          Row {
            anchors.left: parent.left
            anchors.right: actions.left
            anchors.rightMargin: Style.spacing.panelGap
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.md

            Rectangle {
              anchors.verticalCenter: parent.verticalCenter
              visible: root.saving || root.undoing || saveTimer.running
              width: Style.space(8)
              height: width
              radius: width / 2
              color: Color.accent
            }

            Label {
              anchors.verticalCenter: parent.verticalCenter
              width: parent.width - Style.space(20)
              readonly property string daemonProblem: Model.daemonProblem(root.daemon)
              readonly property bool busy: root.saving || root.undoing || saveTimer.running
              text: root.saving ? "Saving…"
                : root.undoing ? "Undoing…"
                : saveTimer.running ? "Saving in a moment…"
                : root.notice !== "" ? root.notice
                : daemonProblem
              color: !busy && ((root.notice !== "" && root.noticeIsError) || (root.notice === "" && daemonProblem !== ""))
                ? Color.urgent : Color.menu.text
            }
          }

          Row {
            id: actions
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.md

            Label {
              anchors.verticalCenter: parent.verticalCenter
              opacity: 0.55
              text: "↑↓ profile    1 2 3 view    ⏎ activate    ctrl+z undo    esc close"
              font.pixelSize: Style.font.caption
            }

            Button {
              visible: root.undoDepth > 0 || root.dirty
              text: "Undo"
              iconText: "󰕌"
              bordered: true
              enabled: !root.saving && !root.undoing
              opacity: enabled ? 1 : 0.5
              foreground: Color.menu.text
              fontFamily: Style.font.menuFamily
              onClicked: root.undo()
            }
          }
        }

        // ---- Workspace --------------------------------------------------
        Item {
          id: workspace
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: header.bottom
          anchors.topMargin: Style.spacing.panelGap
          anchors.bottom: footer.top
          anchors.bottomMargin: Style.spacing.panelGap

          Label {
            anchors.centerIn: parent
            visible: !root.ready && root.loadError === ""
            opacity: 0.6
            text: "Reading your mouse…"
            font.pixelSize: Style.font.title
          }

          Column {
            anchors.centerIn: parent
            width: Math.min(parent.width, Style.space(560))
            visible: root.loadError !== ""
            spacing: Style.spacing.lg

            Label {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.Wrap
              text: root.loadError
              font.pixelSize: Style.font.title
            }

            Button {
              anchors.horizontalCenter: parent.horizontalCenter
              text: "Try again"
              bordered: true
              foreground: Color.menu.text
              onClicked: root.retry()
            }
          }

          Item {
            anchors.fill: parent
            visible: root.ready && root.loadError === ""

            // Profiles.
            Column {
              id: rail
              width: root.railWidth
              anchors.top: parent.top
              spacing: Style.spacing.xs

              PanelSectionHeader {
                text: "Profiles"
                foreground: Color.menu.text
              }

              Repeater {
                model: root.profiles

                delegate: Button {
                  required property var modelData
                  required property int index
                  width: rail.width
                  leftAlign: true
                  iconText: modelData.active ? "󰄬" : ""
                  text: Model.profileTitle(modelData) + (!modelData.enabled ? "  (off)" : "")
                  selected: index === root.cursor
                  active: modelData.active
                  opacity: modelData.enabled ? 1 : 0.6
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.selectProfile(index)
                }
              }

              Label {
                width: rail.width
                visible: root.anyDisabled
                topPadding: Style.spacing.md
                wrapMode: Text.Wrap
                opacity: 0.5
                text: "Profiles marked off are turned off on the mouse. You can still edit them."
                font.pixelSize: Style.font.caption
              }
            }

            Rectangle {
              id: railDivider
              anchors.left: rail.right
              anchors.leftMargin: Style.spacing.lg
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              width: Style.normalBorderWidth
              color: Util.alpha(Color.menu.border, 0.28)
            }

            Item {
              id: stageArea
              anchors.left: railDivider.right
              anchors.leftMargin: Style.spacing.lg
              anchors.right: parent.right
              anchors.top: parent.top
              anchors.bottom: parent.bottom

              // Profile title, views and activation.
              Item {
                id: toolbar
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                height: Math.max(titleColumn.height, toolbarActions.height)

                Column {
                  id: titleColumn
                  anchors.left: parent.left
                  anchors.right: toolbarActions.left
                  anchors.rightMargin: Style.spacing.lg
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.spacing.xxs

                  Label {
                    width: parent.width
                    text: root.selected ? Model.profileTitle(root.selected) : ""
                    font.pixelSize: Style.font.heading
                    font.bold: true
                  }

                  Label {
                    width: parent.width
                    opacity: 0.6
                    text: {
                      if (!root.selected) return ""
                      var note = Model.daemonNote(root.daemon, root.selected)
                      return Model.profileStatus(root.selected) + (note !== "" ? "  ·  " + note : "")
                    }
                  }
                }

                Row {
                  id: toolbarActions
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.spacing.lg

                  ButtonGroup {
                    anchors.verticalCenter: parent.verticalCenter
                    options: [
                      { value: "buttons", label: "Buttons" },
                      { value: "gshift", label: "G-Shift" },
                      { value: "sensitivity", label: "Sensitivity" }
                    ]
                    value: root.tab
                    foreground: Color.menu.text
                    fontFamily: Style.font.menuFamily
                    onChanged: function(value) { root.tab = value }
                  }

                  Button {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Activate"
                    bordered: true
                    enabled: root.selected !== null && root.selected.enabled && !root.selected.active
                    opacity: enabled ? 1 : 0.4
                    foreground: Color.menu.text
                    fontFamily: Style.font.menuFamily
                    onClicked: root.activate()
                  }
                }
              }

              Item {
                id: content
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: toolbar.bottom
                anchors.topMargin: Style.spacing.xxl
                anchors.bottom: parent.bottom

                DeviceCanvas {
                  anchors.left: parent.left
                  anchors.right: inspectorDivider.left
                  anchors.rightMargin: Style.spacing.lg
                  anchors.top: parent.top
                  anchors.bottom: parent.bottom
                  visible: root.tab !== "sensitivity"
                  views: root.views
                  entries: root.entries
                  selectedSlot: root.selectedSlot
                  hoveredSlot: root.hoveredSlot
                  onSlotSelected: function(slot) {
                    root.selectedSlot = slot
                    keyCatcher.forceActiveFocus()
                  }
                  onSlotHovered: function(slot, hovered) { root.hoverSlot(slot, hovered) }
                }

                Rectangle {
                  id: inspectorDivider
                  anchors.right: inspector.left
                  anchors.rightMargin: Style.spacing.lg
                  anchors.top: parent.top
                  anchors.bottom: parent.bottom
                  width: Style.normalBorderWidth
                  visible: root.tab !== "sensitivity"
                  color: Util.alpha(Color.menu.border, 0.28)
                }

                ActionInspector {
                  id: inspector
                  anchors.right: parent.right
                  anchors.top: parent.top
                  anchors.bottom: parent.bottom
                  width: root.inspectorWidth
                  visible: root.tab !== "sensitivity"
                  entry: root.selectedEntry
                  catalog: root.catalog
                  onChosen: function(action) { root.choose(action) }
                  onReverted: root.revertSlot()
                  onRecordingChanged: if (!recording) keyCatcher.forceActiveFocus()
                }

                SensitivityPanel {
                  anchors.fill: parent
                  visible: root.tab === "sensitivity"
                  draft: root.draft
                  bounds: Model.dpiBounds(root.info)
                  rates: root.info && root.info.report_rates_hz ? root.info.report_rates_hz : []
                  onEdited: function(next, immediate) { root.updateDraft(next, immediate) }
                }
              }
            }
          }
        }
      }
    }
  }
}
