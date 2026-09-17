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
  // The helper's `kind` for loadError, when it names one (see Model.setupState).
  property string loadErrorKind: ""
  // The helper answered but is older than this plugin needs.
  property bool helperOutdated: false
  // How far the connected mouse is verified (Model.support); untested mice ask once
  // before the first save.
  property var support: null
  property bool acceptOpen: false
  readonly property var supportBadge: Model.supportBadge(root.support)
  // What stands between the plugin and the mouse (see Model.setupState), or "".
  readonly property string setupKind: root.helperOutdated ? "outdated" : Model.setupState(root.loadError, root.loadErrorKind)
  property string notice: ""
  property bool noticeIsError: false
  property int cursor: 0

  // "buttons" or "gshift" (the Assignments page, by layer), or "sensitivity".
  property string tab: "buttons"
  // Which picture view the Assignments page shows.
  property int viewIndex: 0
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
  readonly property bool assignments: root.tab !== "sensitivity"
  readonly property string table: root.tab === "gshift" ? "gshift" : "buttons"
  readonly property var entries: Model.slotEntries(
    root.selected, root.draft, root.original, root.catalog, root.table, root.buttonCount, root.slotsVerified)
  readonly property var selectedEntry: {
    var entry = Model.indexBySlot(root.entries)[root.selectedSlot]
    return entry === undefined ? null : entry
  }
  readonly property var profileOptions: root.profiles.map(function(slot) {
    var status = slot.active ? "In use" : (slot.enabled ? "" : "Off")
    return { value: String(slot.position), label: Model.profileTitle(slot) + (status !== "" ? "  ·  " + status : "") }
  })

  readonly property int cardWidth: Math.min(Style.space(1440), panel.width - Style.gapsOut * 2)
  readonly property int cardHeight: Math.min(Style.space(860), panel.height - Style.gapsOut * 2)
  readonly property int headerHeight: Math.max(Style.space(44), Style.spacing.controlHeight + Style.spacing.sm)
  readonly property int railWidth: Style.space(52)
  readonly property int libraryWidth: Style.space(330)

  onSelectedSlotChanged: {
    // Show the view that has the selected button.
    var view = Model.viewForSlot(root.views, root.selectedSlot)
    if (view !== -1) root.viewIndex = view
  }

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
    library.stopRecording()
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
    root.loadErrorKind = ""
    server.request({ cmd: "state" }, function(ok, result, kind) {
      root.loading = false
      if (!ok) {
        root.failLoad(result, kind)
        return
      }
      var first = root.onboard === null
      root.helperOutdated = Model.helperOutdated(result)
      root.support = Model.support(result)
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
    root.loadErrorKind = ""
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

  function assign(slot, action) {
    if (!root.draft || slot < 0 || !action) return
    root.selectedSlot = slot
    root.updateDraft(Model.setBinding(root.draft, root.table, slot, action), true)
  }

  function revertSlot(slot) {
    if (!root.draft || slot < 0) return
    root.updateDraft(Model.setBinding(root.draft, root.table, slot, root.original[root.table][slot]), true)
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
    // An untested mouse is written only after the user accepts it, once.
    if (Model.needsAcceptance(root.support)) {
      root.acceptOpen = true
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

  function switchView(step) {
    if (root.views.length < 2) return
    root.viewIndex = (root.viewIndex + step + root.views.length) % root.views.length
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
  function failLoad(message, kind) {
    if (root.ready) {
      root.say(message, true)
      return
    }
    root.loadError = message
    root.loadErrorKind = kind || ""
  }

  // The installer that came with this plugin, beside manifest.json.
  readonly property string installScript: Model.localPath(Qt.resolvedUrl("../install.sh"))

  // Runs the plugin's own installer in Omarchy's floating terminal, as the bar's update
  // button runs omarchy-update, so the password prompt and progress stay in plain sight.
  function installHelper() {
    Util.execArgv(Model.helperInstallArgv(root.installScript))
    root.say("Finish the installer in the terminal, then choose Try again.", false)
  }

  // The setup card's one step: repairing the profile list happens here, everything else
  // in the installer.
  function runSetupAction() {
    if (root.setupKind === "directory") root.repairDirectory()
    else root.installHelper()
  }

  // Rebuilds a damaged profile directory, then reads the mouse again. A directory the
  // helper will not rebuild keeps the card, with the reason and the way to restore.
  function repairDirectory() {
    if (root.loading) return
    root.loading = true
    server.request({ cmd: "repair_directory" }, function(ok, result, kind) {
      root.loading = false
      if (!ok) {
        root.loadError = result
        root.loadErrorKind = kind || ""
        return
      }
      root.loadError = ""
      root.loadErrorKind = ""
      root.refresh()
      root.say("Repaired the profile list. Backup saved to " + result.backup, false)
    })
  }

  // Accepts editing this untested mouse, then saves the changes that were waiting.
  function acceptUntested() {
    root.acceptOpen = false
    server.request({ cmd: "accept_untested" }, function(ok, result) {
      if (!ok) {
        root.say("Not saved: " + result, true)
        return
      }
      root.support = result.support
      root.save()
    })
  }

  // Opens the new-device issue form, prefilled with the model and USB id.
  function reportDevice() {
    Qt.openUrlExternally(Model.reportUrl(root.info, root.support))
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
  // shown as labels alone.
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

  // A page in the icon rail.
  component RailButton: Rectangle {
    id: railButton
    property string icon: ""
    property string label: ""
    property bool selected: false
    signal activated()

    width: root.railWidth - Style.spacing.sm
    height: width
    radius: Style.cornerRadius
    color: selected ? Util.alpha(Color.accent, 0.18) : (railArea.containsMouse ? Util.alpha(Color.menu.text, 0.08) : "transparent")

    Rectangle {
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
      width: Math.max(2, Style.space(3))
      height: parent.height * 0.5
      radius: width / 2
      visible: railButton.selected
      color: Color.accent
    }

    Icon {
      anchors.centerIn: parent
      name: railButton.icon
      tint: railButton.selected ? Color.accent : Color.menu.text
      size: Style.space(22)
    }

    MouseArea {
      id: railArea
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: railButton.activated()
    }

    PanelToolTip {
      visible: railArea.containsMouse
      text: railButton.label
    }
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
          } else if (event.key === Qt.Key_Left || event.text === "h") {
            root.switchView(-1)
          } else if (event.key === Qt.Key_Right || event.text === "l") {
            root.switchView(1)
          } else if (event.text === "g") {
            root.tab = root.tab === "gshift" ? "buttons" : "gshift"
          } else if (event.text === "1") {
            if (!root.assignments) root.tab = "buttons"
          } else if (event.text === "2") {
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

        // ---- Header: brand, profile, activation, device ---------------------
        Item {
          id: header
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          height: root.headerHeight

          Row {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.lg

            Row {
              anchors.verticalCenter: parent.verticalCenter
              spacing: Style.spacing.sm

              // The wheel rolls while a change is being written to the mouse.
              PixelMark {
                anchors.verticalCenter: parent.verticalCenter
                width: Style.space(32)
                height: width
                busy: root.saving || root.undoing
              }

              Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "Omalogi"
                font.pixelSize: Style.font.heading
                font.bold: true
              }
            }

            Dropdown {
              anchors.verticalCenter: parent.verticalCenter
              width: Style.space(260)
              showLabel: false
              visible: root.ready
              options: root.profileOptions
              value: String(root.cursor)
              fontFamily: Style.font.menuFamily
              onChanged: function(value) { root.selectProfile(Number(value)) }
            }

            // The profile list already says which profile is in use, so the button only
            // shows when there is something to do.
            Button {
              anchors.verticalCenter: parent.verticalCenter
              visible: root.ready && !(root.selected && root.selected.active)
              text: "Activate"
              bordered: true
              enabled: root.selected !== null && root.selected.enabled && !root.selected.active
              opacity: enabled ? 1 : 0.5
              foreground: Color.menu.text
              fontFamily: Style.font.menuFamily
              onClicked: root.activate()
            }

            Label {
              anchors.verticalCenter: parent.verticalCenter
              visible: root.ready
              opacity: 0.55
              text: {
                if (!root.selected) return ""
                var note = Model.daemonNote(root.daemon, root.selected)
                return note !== "" ? note : (root.selected.active ? "" : Model.profileStatus(root.selected))
              }
              font.pixelSize: Style.font.caption
            }
          }

          Row {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.md

            // An untested mouse says so, and opens the Mouse report form to verify it or report how it went.
            Rectangle {
              anchors.verticalCenter: parent.verticalCenter
              visible: root.supportBadge !== null
              width: badgeLabel.implicitWidth + Style.spacing.md * 2
              height: badgeLabel.implicitHeight + Style.spacing.xs * 2
              radius: height / 2
              color: badgeArea.containsMouse ? Util.alpha(Color.accent, 0.16) : "transparent"
              border.width: Math.max(1, Style.normalBorderWidth)
              border.color: Color.accent

              Label {
                id: badgeLabel
                anchors.centerIn: parent
                text: root.supportBadge ? root.supportBadge.text + "  ·  " + root.supportBadge.action : ""
                color: Color.accent
                font.pixelSize: Style.font.caption
              }

              MouseArea {
                id: badgeArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.reportDevice()
                onContainsMouseChanged: if (containsMouse && root.supportBadge) root.say(root.supportBadge.detail, false)
              }
            }

            Label {
              anchors.verticalCenter: parent.verticalCenter
              horizontalAlignment: Text.AlignRight
              opacity: 0.55
              text: root.info ? Model.deviceSummary(root.info) : ""
            }
          }
        }

        // ---- Untested mouse: accept once before the first write -------------
        Rectangle {
          anchors.fill: parent
          z: 100
          visible: root.acceptOpen
          color: Util.alpha(Color.menu.background, 0.82)

          // A click outside the dialog closes it without saving.
          MouseArea {
            anchors.fill: parent
            onClicked: root.acceptOpen = false
          }

          Rectangle {
            anchors.centerIn: parent
            width: Math.min(parent.width - Style.space(48), Style.space(520))
            height: acceptColumn.implicitHeight + Style.spacing.lg * 2
            radius: Style.cornerRadius
            color: Color.menu.background
            border.width: Math.max(1, Style.normalBorderWidth)
            border.color: Util.alpha(Color.accent, 0.7)

            // Keeps clicks inside the dialog from closing it.
            MouseArea { anchors.fill: parent }

            Column {
              id: acceptColumn
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.top: parent.top
              anchors.margins: Style.spacing.lg
              spacing: Style.spacing.lg

              PixelMark {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Style.space(56)
                height: width
              }

              Label {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                text: root.support ? "Edit your " + root.support.name + "?" : ""
                font.pixelSize: Style.font.title
                font.bold: true
              }

              Label {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                opacity: 0.75
                text: "Omalogi has not been tested on this model yet. It uses the same onboard memory as the verified G502 X, and every change is backed up first and read back to check it. You only need to accept this once."
              }

              Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: Style.spacing.md

                Button {
                  text: "Edit it"
                  bordered: true
                  foreground: Color.accent
                  onClicked: root.acceptUntested()
                }

                Button {
                  text: "Not now"
                  bordered: true
                  foreground: Color.menu.text
                  onClicked: root.acceptOpen = false
                }
              }

              Label {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                opacity: 0.5
                text: "After trying it, please tell us how it went with the Untested badge at the top."
                font.pixelSize: Style.font.caption
              }
            }
          }
        }

        // ---- Footer: save status and Undo ------------------------------------
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

            // Editor keys only mean something once the editor is showing.
            Label {
              anchors.verticalCenter: parent.verticalCenter
              visible: root.ready && root.setupKind === ""
              opacity: 0.5
              text: root.assignments
                ? "2 Sensitivity    ←→ view    g G-Shift    ↑↓ profile    ctrl+z undo    esc close"
                : "1 Assignments    ↑↓ profile    ctrl+z undo    esc close"
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

        // ---- Workspace ----------------------------------------------------------
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
            visible: !root.ready && root.setupKind === ""
            opacity: 0.6
            text: "Reading your mouse…"
            font.pixelSize: Style.font.title
          }

          // What stands between the plugin and the mouse, and the one step that fixes it.
          Column {
            id: setupCard
            readonly property var copy: Model.setupCopy(root.setupKind, root.loadError)
            anchors.centerIn: parent
            width: Math.min(parent.width, Style.space(520))
            visible: root.setupKind !== ""
            spacing: Style.spacing.lg

            PixelMark {
              anchors.horizontalCenter: parent.horizontalCenter
              width: Style.space(64)
              height: width
              busy: root.loading
            }

            Label {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.Wrap
              text: setupCard.copy.title
              font.pixelSize: Style.font.title
              font.bold: true
            }

            Label {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.Wrap
              opacity: 0.7
              text: setupCard.copy.body
            }

            Row {
              anchors.horizontalCenter: parent.horizontalCenter
              spacing: Style.spacing.md

              Button {
                visible: setupCard.copy.action !== ""
                text: setupCard.copy.action
                bordered: true
                foreground: Color.accent
                onClicked: root.runSetupAction()
              }

              Button {
                text: "Try again"
                bordered: true
                foreground: Color.menu.text
                onClicked: root.retry()
              }
            }

            Label {
              width: parent.width
              visible: setupCard.copy.installer
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.WrapAnywhere
              opacity: 0.45
              text: "Opens a terminal and runs  " + Model.helperInstallCommand(root.installScript)
              font.pixelSize: Style.font.caption
            }
          }

          Item {
            id: pages
            anchors.fill: parent
            visible: root.ready && root.setupKind === ""

            // Pages.
            Column {
              id: rail
              width: root.railWidth
              anchors.top: parent.top
              spacing: Style.spacing.sm

              RailButton {
                icon: "mouse-pointer-click"
                label: "Assignments"
                selected: root.assignments
                onActivated: if (!root.assignments) root.tab = "buttons"
              }

              RailButton {
                icon: "gauge"
                label: "Sensitivity"
                selected: !root.assignments
                onActivated: root.tab = "sensitivity"
              }
            }

            Rectangle {
              id: railDivider
              anchors.left: rail.right
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              width: Style.normalBorderWidth
              color: Util.alpha(Color.menu.border, 0.28)
            }

            // Assignments: the library, and the mouse.
            Item {
              anchors.left: railDivider.right
              anchors.leftMargin: Style.spacing.lg
              anchors.right: parent.right
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              visible: root.assignments

              ActionLibrary {
                id: library
                width: root.libraryWidth
                anchors.left: parent.left
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                catalog: root.catalog
                entries: root.entries
                entry: root.selectedEntry
                layerName: root.table === "gshift" ? "G-Shift layer" : "Default layer"
                gshift: root.table === "gshift"
                gshiftButtons: Model.gshiftButtons(root.draft, root.buttonCount)
                dragProxy: dragProxy
                onChosen: function(action) { root.assign(root.selectedSlot, action) }
                onShortcutRecorded: function(slot, action) { root.assign(slot, action) }
                onSelectionNeeded: root.say("Select a button on the mouse first, or drag the action onto one.", false)
                onRecordingChanged: if (!recording) keyCatcher.forceActiveFocus()
              }

              Rectangle {
                id: libraryDivider
                anchors.left: library.right
                anchors.leftMargin: Style.spacing.lg
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                width: Style.normalBorderWidth
                color: Util.alpha(Color.menu.border, 0.28)
              }

              DeviceCanvas {
                anchors.left: libraryDivider.right
                anchors.leftMargin: Style.spacing.lg
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                views: root.views
                viewIndex: root.viewIndex
                entries: root.entries
                catalog: root.catalog
                factory: root.onboard ? root.onboard.factory : null
                table: root.table
                selectedSlot: root.selectedSlot
                hoveredSlot: root.hoveredSlot
                onSlotSelected: function(slot) {
                  root.selectedSlot = slot
                  keyCatcher.forceActiveFocus()
                }
                onSlotHovered: function(slot, hovered) { root.hoverSlot(slot, hovered) }
                onViewRequested: function(index) { root.viewIndex = index }
                onLayerRequested: function(gshift) { root.tab = gshift ? "gshift" : "buttons" }
                onActionAssigned: function(slot, action) { root.assign(slot, action) }
                onRecordRequested: function(slot) { library.startRecording(slot) }
                onRevertRequested: function(slot) { root.revertSlot(slot) }
              }
            }

            SensitivityPanel {
              anchors.left: railDivider.right
              anchors.leftMargin: Style.space(40)
              anchors.top: parent.top
              anchors.topMargin: Style.spacing.md
              anchors.bottom: parent.bottom
              width: Math.min(parent.width - root.railWidth - Style.space(80), Style.space(1000))
              visible: !root.assignments
              draft: root.draft
              bounds: Model.dpiBounds(root.info)
              rates: root.info && root.info.report_rates_hz ? root.info.report_rates_hz : []
              onEdited: function(next, immediate) { root.updateDraft(next, immediate) }
            }

            // Follows the pointer while an action is dragged from the library.
            Rectangle {
              id: dragProxy
              property string action: ""
              property string label: ""
              property bool dragging: false

              function prepare(row, x, y) {
                dragProxy.action = row.value
                dragProxy.label = row.label
                dragProxy.x = x - dragProxy.width / 2
                dragProxy.y = y - dragProxy.height / 2
              }

              visible: dragging
              z: 100
              width: dragRow.implicitWidth + Style.spacing.md * 2
              height: Style.space(36)
              radius: Style.cornerRadius
              color: Color.menu.background
              border.width: Math.max(2, Style.normalBorderWidth)
              border.color: Color.accent
              Drag.active: dragging
              Drag.keys: ["omalogi-action"]
              Drag.hotSpot.x: width / 2
              Drag.hotSpot.y: height / 2

              Row {
                id: dragRow
                anchors.centerIn: parent
                spacing: Style.spacing.sm

                Icon {
                  anchors.verticalCenter: parent.verticalCenter
                  name: Model.actionIcon(dragProxy.action)
                  tint: Color.accent
                  size: Style.space(18)
                }

                Label {
                  anchors.verticalCenter: parent.verticalCenter
                  text: dragProxy.label
                }
              }
            }
          }
        }
      }
    }
  }
}
