import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Omalogi's overlay: the connected mouse's onboard profiles, edited the way G HUB does
// it. Every device read and write goes through the omalogi CLI, one command at a time
// (they share a HID++ software id), so the plugin never touches the device itself.
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
  // The selected profile as read from the mouse, and with the unsaved edits.
  property var original: null
  property var draft: null
  property int selectedSlot: -1
  property int hoveredSlot: -1
  // Set by a write, so the next profile read replaces the draft.
  property bool reloadDraft: false
  // What the open payload asked for, applied once profiles have loaded.
  property var pendingOpen: null

  // The open confirmation: "apply", "switch" (to `pendingCursor`), "close", or "".
  property string confirmMode: ""
  property int pendingCursor: -1
  property string previewText: ""
  property string applyError: ""

  readonly property var profiles: root.onboard ? root.onboard.profiles : []
  readonly property var selected: root.profiles.length > 0
    ? root.profiles[Model.clampCursor(root.cursor, root.profiles.length)]
    : null
  // Device commands only; they must never overlap.
  readonly property bool busy: infoCommand.running || profilesCommand.running || activateCommand.running
    || previewCommand.running || saveCommand.running
  readonly property bool ready: root.info !== null && root.onboard !== null
  readonly property int changes: Model.changeCount(root.draft, root.original)
  readonly property bool dirty: root.changes > 0
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
  readonly property string unreadable: "omalogi answered with output Omalogi could not read."

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
    if (root.dirty) {
      root.ask("close")
      return
    }
    root.opened = false
    enterAnimation.stop()
    exitAnimation.restart()
  }

  function finishClose() {
    root.mounted = false
    root.confirmMode = ""
    root.selectedSlot = -1
    if (root.shell && root.manifest) root.shell.hide(root.manifest.id)
  }

  function refresh() {
    if (root.busy) return
    root.loadError = ""
    infoCommand.start(["info", "--json"])
  }

  function applyOpenRequest() {
    var request = root.pendingOpen
    if (request === null || !root.ready) return
    root.pendingOpen = null
    if (request.profile !== null) root.selectProfile(request.profile - 1, true)
    if (request.tab !== null) root.tab = request.tab
    if (request.button !== null) root.selectedSlot = request.button
  }

  function loadDraft() {
    root.original = root.selected ? Model.draftFromSlot(root.selected) : null
    root.draft = root.original
    root.applyError = ""
  }

  // Selects a profile. Unsaved edits ask first, unless `discard` is set.
  function selectProfile(index, discard) {
    var next = Model.clampCursor(index, root.profiles.length)
    if (next === root.cursor && root.draft !== null) return
    if (root.dirty && !discard) {
      root.pendingCursor = next
      root.ask("switch")
      return
    }
    root.cursor = next
    root.loadDraft()
  }

  function ask(mode) {
    root.confirmMode = mode
    keyCatcher.forceActiveFocus()
  }

  function confirm() {
    var mode = root.confirmMode
    root.confirmMode = ""
    keyCatcher.forceActiveFocus()
    if (mode === "apply") {
      root.write()
    } else if (mode === "switch") {
      root.revertAll()
      root.selectProfile(root.pendingCursor, true)
    } else if (mode === "close") {
      root.revertAll()
      root.close()
    }
  }

  function updateDraft(next) {
    root.draft = next
    root.applyError = ""
  }

  function revertAll() {
    root.draft = root.original
    root.applyError = ""
  }

  function choose(action) {
    if (!root.draft || root.selectedSlot < 0) return
    root.updateDraft(Model.setBinding(root.draft, root.table, root.selectedSlot, action))
  }

  function revertSlot() {
    if (!root.draft || root.selectedSlot < 0) return
    var saved = root.original[root.table][root.selectedSlot]
    root.updateDraft(Model.setBinding(root.draft, root.table, root.selectedSlot, saved))
  }

  // Apply: a dry run of exactly these changes, shown in the confirmation, then the write.
  function apply() {
    if (!root.dirty || root.busy) return
    var problem = Model.draftProblem(root.draft)
    if (problem !== "") {
      root.applyError = problem
      return
    }
    root.applyError = ""
    previewCommand.start(Model.editArgs(root.draft, root.original, true))
  }

  function write() {
    if (!root.dirty || root.busy) return
    saveCommand.start(Model.editArgs(root.draft, root.original, false).concat(["--json"]))
  }

  function activate() {
    var slot = root.selected
    if (!slot || root.busy) return
    var refusal = Model.activationRefusal(slot)
    if (refusal !== "") {
      root.say(refusal, false)
      return
    }
    root.say("", false)
    activateCommand.start(["profiles", "activate", String(slot.position + 1), "--json"])
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
    // The daemon switched profiles while the overlay is open: show the new active one.
    var switched = state !== null && (previous === null || previous.active_profile !== state.active_profile)
    if (root.opened && root.ready && switched && !root.busy) profilesCommand.start(["profiles", "--json"])
  }

  OmalogiCommand {
    id: infoCommand
    onFinished: function(exitCode, stdout, stderr) {
      var parsed = exitCode === 0 ? Model.parseJson(stdout) : null
      if (parsed === null) {
        root.failLoad(exitCode === 0 ? root.unreadable : Model.errorMessage(stderr, exitCode))
        return
      }
      root.info = parsed
      profilesCommand.start(["profiles", "--json"])
    }
  }

  OmalogiCommand {
    id: profilesCommand
    onFinished: function(exitCode, stdout, stderr) {
      var parsed = exitCode === 0 ? Model.parseJson(stdout) : null
      if (parsed === null) {
        root.failLoad(exitCode === 0 ? root.unreadable : Model.errorMessage(stderr, exitCode))
        return
      }
      var first = root.onboard === null
      root.onboard = parsed
      if (first) root.cursor = Model.initialCursor(parsed)
      // Unsaved edits survive a refresh; a write replaces them with what the mouse has.
      if (first || root.reloadDraft || !root.dirty) {
        root.reloadDraft = false
        root.loadDraft()
      }
      Qt.callLater(root.applyOpenRequest)
    }
  }

  OmalogiCommand {
    id: activateCommand
    onFinished: function(exitCode, stdout, stderr) {
      var result = exitCode === 0 ? Model.parseJson(stdout) : null
      if (result === null) {
        root.say(exitCode === 0 ? root.unreadable : Model.errorMessage(stderr, exitCode), true)
      } else {
        root.say("Profile " + result.active_profile + " is now active.", false)
      }
      profilesCommand.start(["profiles", "--json"])
    }
  }

  // Needs no device, so it may run alongside device commands.
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

  OmalogiCommand {
    id: previewCommand
    onFinished: function(exitCode, stdout, stderr) {
      if (exitCode !== 0) {
        root.applyError = Model.errorMessage(stderr, exitCode)
        return
      }
      root.previewText = stdout.trim()
      root.ask("apply")
    }
  }

  OmalogiCommand {
    id: saveCommand
    onFinished: function(exitCode, stdout, stderr) {
      var report = exitCode === 0 ? Model.parseJson(stdout) : null
      if (report !== null) {
        root.reloadDraft = true
        var notice = Model.savedNotice(report)
        root.say(notice.text, notice.isError)
      } else {
        root.applyError = exitCode === 0 ? root.unreadable : Model.errorMessage(stderr, exitCode)
      }
      profilesCommand.start(["profiles", "--json"])
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
          if (confirmDialog.handleKey(event)) {
            event.accepted = true
            return
          }
          var ctrl = (event.modifiers & Qt.ControlModifier) !== 0
          if (event.key === Qt.Key_Escape) {
            if (root.selectedSlot >= 0) root.selectedSlot = -1
            else root.close()
          } else if (ctrl && event.key === Qt.Key_S) {
            root.apply()
          } else if (event.key === Qt.Key_Down || event.text === "j") {
            root.selectProfile(root.cursor + 1, false)
          } else if (event.key === Qt.Key_Up || event.text === "k") {
            root.selectProfile(root.cursor - 1, false)
          } else if (event.text === "1") {
            root.tab = "buttons"
          } else if (event.text === "2") {
            root.tab = "gshift"
          } else if (event.text === "3") {
            root.tab = "sensitivity"
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            root.activate()
          } else if (event.text === "r" && !root.dirty) {
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

        // ---- Footer -----------------------------------------------------
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
              visible: root.dirty && root.applyError === ""
              width: Style.space(8)
              height: width
              radius: width / 2
              color: Color.accent
            }

            Label {
              anchors.verticalCenter: parent.verticalCenter
              width: parent.width - Style.space(20)
              readonly property string daemonProblem: Model.daemonProblem(root.daemon)
              text: root.applyError !== ""
                ? root.applyError
                : root.dirty
                  ? root.changes + (root.changes === 1 ? " unsaved change" : " unsaved changes")
                    + " to profile " + (root.draft ? root.draft.number : "")
                  : (root.notice !== "" ? root.notice : daemonProblem)
              color: root.applyError !== "" || (!root.dirty && root.notice !== "" && root.noticeIsError)
                || (!root.dirty && root.notice === "" && daemonProblem !== "")
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
              visible: !root.dirty
              opacity: 0.55
              text: "↑↓ profile    1 2 3 view    ⏎ activate    r refresh    esc close"
              font.pixelSize: Style.font.caption
            }

            Button {
              visible: root.dirty
              text: "Revert"
              bordered: true
              enabled: !saveCommand.running
              foreground: Color.menu.text
              fontFamily: Style.font.menuFamily
              onClicked: root.revertAll()
            }

            Button {
              visible: root.dirty
              text: previewCommand.running ? "Checking…" : (saveCommand.running ? "Writing…" : "Apply to mouse")
              bordered: true
              active: true
              enabled: !root.busy
              foreground: Color.menu.text
              fontFamily: Style.font.menuFamily
              onClicked: root.apply()
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
              onClicked: root.refresh()
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
                  onClicked: root.selectProfile(index, false)
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
                    text: activateCommand.running ? "Activating…" : "Activate"
                    bordered: true
                    enabled: root.selected !== null && root.selected.enabled && !root.selected.active && !root.busy
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
                  onEdited: function(next) { root.updateDraft(next) }
                }
              }
            }
          }
        }
      }

      ConfirmDialog {
        id: confirmDialog
        anchors.fill: parent
        z: 10
        opened: root.confirmMode !== ""
        background: Color.menu.background
        foreground: Color.menu.text
        fontFamily: Style.font.menuFamily
        message: {
          var number = root.draft ? root.draft.number : ""
          if (root.confirmMode === "apply") {
            return "Write " + root.changes + (root.changes === 1 ? " change" : " changes") + " to profile " + number + "?\n\n"
              + root.previewText
              + "\n\n" + (root.selected ? Model.applyNote(root.selected) + " " : "")
              + "All profiles are backed up first, and the write is read back to verify it."
          }
          if (root.confirmMode === "switch") return "Discard the unsaved changes to profile " + number + "?"
          return "Close and discard the unsaved changes to profile " + number + "?"
        }
        confirmText: root.confirmMode === "apply" ? "Write to mouse" : "Discard"
        cancelText: root.confirmMode === "apply" ? "Cancel" : "Keep editing"
        onCanceled: {
          root.confirmMode = ""
          keyCatcher.forceActiveFocus()
        }
        onConfirmed: root.confirm()
      }
    }
  }
}
