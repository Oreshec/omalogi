import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Omalogi's overlay: the connected mouse and its onboard profiles, with editing.
// Every device read and write goes through the omalogi CLI, one command at a time
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
  property string loadError: ""
  property string notice: ""
  property bool noticeIsError: false
  property int cursor: 0
  // The daemon's published state, or null when it is not running.
  property var daemon: null

  // Editing: `draft` is replaced on every change; `original` is the profile as read.
  property bool editing: false
  property var draft: null
  property var original: null
  property var catalog: []
  property string editTab: "dpi"
  property string previewText: ""
  property bool previewIsError: false
  property bool previewOk: false
  property bool confirmOpen: false
  // What the open payload asked for, applied once profiles have loaded.
  property var pendingOpen: null
  // `omalogi picture`: the mouse's picture with button positions, or null.
  property var picture: null
  // The slot under the pointer, in the picture or the binding table, or -1.
  property int hoveredSlot: -1

  readonly property var profiles: root.onboard ? root.onboard.profiles : []
  readonly property var selected: root.profiles.length > 0
    ? root.profiles[Model.clampCursor(root.cursor, root.profiles.length)]
    : null
  // Device commands only; they must never overlap.
  readonly property bool busy: infoCommand.running || profilesCommand.running || activateCommand.running
    || previewCommand.running || saveCommand.running
  readonly property bool ready: root.info !== null && root.onboard !== null
  readonly property string unreadable: "omalogi answered with output Omalogi could not read."
  readonly property string footerText: root.notice !== "" ? root.notice : Model.daemonProblem(root.daemon)
  readonly property bool footerIsError: root.notice !== "" ? root.noticeIsError : root.footerText !== ""
  readonly property var dpiBounds: Model.dpiBounds(root.info)
  readonly property int buttonCount: root.onboard ? root.onboard.description.button_count : 0
  readonly property string editTable: root.editTab === "gshift" ? "gshift" : "buttons"

  readonly property int cardWidth: Math.min(Style.space(1040), panel.width - Style.gapsOut * 2)
  readonly property int pictureHeight: Style.space(330)
  readonly property int cardHeight: Math.min(Style.space(640), panel.height - Style.gapsOut * 2)
  readonly property int headerHeight: Math.max(Style.space(40), Style.font.heading + Style.spacing.controlPaddingY * 2)
  readonly property int listWidth: Style.space(220)
  readonly property int slotColumnWidth: Style.space(56)

  // payloadJson may name a profile to select and open in the editor, for keybindings:
  // {"profile": 3, "edit": true, "tab": "buttons"}.
  function open(payloadJson) {
    exitAnimation.stop()
    root.mounted = true
    root.opened = true
    enterAnimation.restart()
    root.pendingOpen = Model.openRequest(Model.parseJson(payloadJson))
    if (root.ready && !root.busy) root.applyOpenRequest()
    root.refresh()
    if (root.picture === null && !pictureCommand.running) pictureCommand.start(["picture", "--json"])
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    if (!root.mounted || !root.opened) return
    root.opened = false
    enterAnimation.stop()
    exitAnimation.restart()
  }

  function finishClose() {
    root.mounted = false
    root.stopEditing()
    if (root.shell && root.manifest) root.shell.hide(root.manifest.id)
  }

  function applyOpenRequest() {
    var request = root.pendingOpen
    if (request === null || !root.ready) return
    root.pendingOpen = null
    if (request.profile !== null) root.cursor = Model.clampCursor(request.profile - 1, root.profiles.length)
    if (request.edit) {
      root.startEditing()
      if (root.editing) root.editTab = request.tab
    }
  }

  function hoverSlot(slot, hovered) {
    if (hovered) root.hoveredSlot = slot
    else if (root.hoveredSlot === slot) root.hoveredSlot = -1
  }

  function refresh() {
    if (root.busy) return
    root.loadError = ""
    infoCommand.start(["info", "--json"])
  }

  function moveCursor(delta) {
    root.cursor = Model.clampCursor(root.cursor + delta, root.profiles.length)
  }

  function activate(position) {
    var slot = root.profiles[position]
    if (!slot || root.busy) return
    root.cursor = position
    var refusal = Model.activationRefusal(slot)
    if (refusal !== "") {
      root.say(refusal, false)
      return
    }
    root.say("", false)
    activateCommand.start(["profiles", "activate", String(slot.position + 1), "--json"])
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
    if (root.opened && root.ready && switched && !root.busy && !root.editing) profilesCommand.start(["profiles", "--json"])
  }

  function startEditing() {
    if (!root.selected || !root.selected.enabled || root.busy || root.editing) return
    root.original = Model.draftFromSlot(root.selected)
    root.draft = root.original
    root.editTab = "dpi"
    root.clearPreview()
    root.say("", false)
    root.editing = true
    if (root.catalog.length === 0 && !catalogCommand.running) catalogCommand.start(["actions", "--json"])
  }

  function stopEditing() {
    root.editing = false
    root.confirmOpen = false
    root.draft = null
    root.original = null
    root.clearPreview()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function clearPreview() {
    root.previewText = ""
    root.previewIsError = false
    root.previewOk = false
  }

  // Every change invalidates the preview, so Save always matches what was previewed.
  function updateDraft(next) {
    root.draft = next
    root.clearPreview()
  }

  function requestPreview() {
    if (!root.draft || root.busy) return
    var problem = Model.draftProblem(root.draft)
    if (problem !== "") {
      root.previewText = problem
      root.previewIsError = true
      return
    }
    if (!Model.hasChanges(root.draft, root.original)) {
      root.previewText = "Nothing has changed yet."
      root.previewIsError = false
      return
    }
    previewCommand.start(Model.editArgs(root.draft, root.original, true))
  }

  function requestSave() {
    if (!root.previewOk || root.busy) return
    root.confirmOpen = true
    keyCatcher.forceActiveFocus()
  }

  function confirmSave() {
    root.confirmOpen = false
    if (!root.previewOk || root.busy) return
    saveCommand.start(Model.editArgs(root.draft, root.original, false).concat(["--json"]))
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

  // The action list needs no device, so it may run alongside device commands.
  OmalogiCommand {
    id: catalogCommand
    onFinished: function(exitCode, stdout, stderr) {
      var parsed = exitCode === 0 ? Model.parseJson(stdout) : null
      if (parsed === null) root.say(Model.errorMessage(stderr, exitCode), true)
      else root.catalog = parsed
    }
  }

  // The picture command reads sysfs and the cache, never the device, so it runs alongside
  // device commands. Without a picture the overlay shows the binding table alone.
  OmalogiCommand {
    id: pictureCommand
    onFinished: function(exitCode, stdout, stderr) {
      root.picture = exitCode === 0 ? Model.parseJson(stdout) : null
    }
  }

  OmalogiCommand {
    id: previewCommand
    onFinished: function(exitCode, stdout, stderr) {
      root.previewOk = exitCode === 0
      root.previewIsError = exitCode !== 0
      root.previewText = exitCode === 0 ? stdout.trim() : Model.errorMessage(stderr, exitCode)
    }
  }

  OmalogiCommand {
    id: saveCommand
    onFinished: function(exitCode, stdout, stderr) {
      var report = exitCode === 0 ? Model.parseJson(stdout) : null
      if (report !== null) {
        root.stopEditing()
        root.say("Profile " + report.profile + " saved and verified. Backup: " + report.backup, false)
      } else {
        root.previewOk = false
        root.previewIsError = true
        root.previewText = exitCode === 0 ? root.unreadable : Model.errorMessage(stderr, exitCode)
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

  component Caption: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    opacity: 0.6
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.caption
  }

  component BindingLabel: Text {
    property var label: null
    textFormat: Text.PlainText
    text: label === null ? "—" : label
    color: Color.menu.text
    opacity: label === null ? 0.4 : 1
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.body
  }

  // Bindings by slot, with the G-Shift column when the profile binds any G-Shift button.
  component BindingTable: Column {
    id: table
    property var rows: []
    property bool showGShift: false
    readonly property int columns: showGShift ? 2 : 1
    readonly property int labelWidth: (width - root.slotColumnWidth - Style.spacing.md * columns) / columns

    spacing: Style.spacing.xxs

    Row {
      spacing: Style.spacing.md

      Item { width: root.slotColumnWidth; height: 1 }

      PanelSectionHeader {
        width: table.labelWidth
        text: "Buttons"
        foreground: Color.menu.text
      }

      PanelSectionHeader {
        width: table.labelWidth
        visible: table.showGShift
        text: "G-Shift"
        foreground: Color.menu.text
      }
    }

    Repeater {
      model: table.rows

      delegate: Rectangle {
        id: row
        required property var modelData
        readonly property bool hot: root.hoveredSlot === modelData.slot
        width: table.width
        height: rowContent.implicitHeight + Style.spacing.xxs * 2
        radius: Style.cornerRadius
        color: hot ? Util.alpha(Color.menu.text, 0.08) : "transparent"

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          onContainsMouseChanged: root.hoverSlot(row.modelData.slot, containsMouse)
        }

        Row {
          id: rowContent
          anchors.verticalCenter: parent.verticalCenter
          spacing: Style.spacing.md

          Caption {
            width: root.slotColumnWidth
            text: "slot " + row.modelData.slot
            color: row.hot ? Color.accent : Color.menu.text
            opacity: row.hot ? 1 : 0.6
            font.pixelSize: Style.font.body
          }

          BindingLabel {
            width: table.labelWidth
            label: row.modelData.button
          }

          BindingLabel {
            width: table.labelWidth
            visible: table.showGShift
            label: row.modelData.gshift
          }
        }
      }
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
      onClicked: if (!root.editing) root.close()
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

      MouseArea { anchors.fill: parent; onClicked: {} }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.onPressed: function(event) {
          if (confirmDialog.handleKey(event)) {
            event.accepted = true
            return
          }
          if (root.editing) {
            if (event.key === Qt.Key_Escape) {
              root.stopEditing()
              event.accepted = true
            }
            return
          }
          if (event.key === Qt.Key_Escape) {
            root.close()
          } else if (event.key === Qt.Key_Down || event.text === "j") {
            root.moveCursor(1)
          } else if (event.key === Qt.Key_Up || event.text === "k") {
            root.moveCursor(-1)
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
            root.activate(root.cursor)
          } else if (event.text === "e") {
            root.startEditing()
          } else if (event.text === "r") {
            root.refresh()
          } else {
            return
          }
          event.accepted = true
        }
      }

      Column {
        anchors.fill: parent
        anchors.topMargin: card.contentTopInset
        anchors.rightMargin: card.contentRightInset
        anchors.bottomMargin: card.contentBottomInset
        anchors.leftMargin: card.contentLeftInset
        spacing: Style.spacing.panelGap

        Item {
          width: parent.width
          height: root.headerHeight

          Text {
            id: brand
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: "Omalogi"
            color: Color.menu.text
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.heading
            font.bold: true
          }

          Text {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, parent.width - brand.width - Style.spacing.panelGap)
            horizontalAlignment: Text.AlignRight
            textFormat: Text.PlainText
            text: root.info ? Model.deviceSummary(root.info) : ""
            color: Color.menu.text
            opacity: 0.6
            elide: Text.ElideRight
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.body
          }
        }

        Item {
          width: parent.width
          height: parent.height - root.headerHeight - footer.height - parent.spacing * 2

          Text {
            anchors.centerIn: parent
            visible: !root.ready && root.loadError === ""
            textFormat: Text.PlainText
            text: "Reading your mouse…"
            color: Color.menu.text
            opacity: 0.6
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.title
          }

          Column {
            anchors.centerIn: parent
            width: Math.min(parent.width, Style.space(560))
            visible: root.loadError !== ""
            spacing: Style.spacing.lg

            Text {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.Wrap
              textFormat: Text.PlainText
              text: root.loadError
              color: Color.menu.text
              font.family: Style.font.menuFamily
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

            Column {
              id: profileList
              width: root.listWidth
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              spacing: Style.spacing.xs
              enabled: !root.editing
              opacity: root.editing ? 0.5 : 1

              PanelSectionHeader {
                text: "Profiles"
                foreground: Color.menu.text
              }

              Repeater {
                model: root.profiles

                delegate: Button {
                  required property var modelData
                  required property int index
                  width: profileList.width
                  leftAlign: true
                  iconText: modelData.active ? "󰄬" : ""
                  text: Model.profileTitle(modelData)
                  selected: index === root.cursor
                  active: modelData.active
                  opacity: modelData.enabled ? 1 : 0.5
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.cursor = index
                }
              }
            }

            Rectangle {
              id: divider
              anchors.left: profileList.right
              anchors.leftMargin: Style.spacing.lg
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              width: Style.normalBorderWidth
              color: Util.alpha(Color.menu.border, 0.28)
            }

            // ---- Profile details -----------------------------------------
            Column {
              anchors.left: divider.right
              anchors.leftMargin: Style.spacing.lg
              anchors.right: parent.right
              anchors.top: parent.top
              spacing: Style.spacing.xxl
              visible: root.selected !== null && !root.editing

              Item {
                width: parent.width
                height: Math.max(titleColumn.height, detailActions.height)

                Column {
                  id: titleColumn
                  anchors.left: parent.left
                  anchors.right: detailActions.left
                  anchors.rightMargin: Style.spacing.lg
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.spacing.xxs

                  Text {
                    width: parent.width
                    textFormat: Text.PlainText
                    text: root.selected ? Model.profileTitle(root.selected) : ""
                    color: Color.menu.text
                    elide: Text.ElideRight
                    font.family: Style.font.menuFamily
                    font.pixelSize: Style.font.heading
                    font.bold: true
                  }

                  Caption {
                    width: parent.width
                    text: root.selected ? Model.profileStatus(root.selected) : ""
                    font.pixelSize: Style.font.body
                  }

                  Caption {
                    width: parent.width
                    visible: text !== ""
                    text: root.selected ? Model.daemonNote(root.daemon, root.selected) : ""
                    font.pixelSize: Style.font.body
                  }
                }

                Row {
                  id: detailActions
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.spacing.md

                  Button {
                    text: "Edit"
                    bordered: true
                    enabled: root.selected !== null && root.selected.enabled && !root.busy
                    opacity: enabled ? 1 : 0.4
                    foreground: Color.menu.text
                    fontFamily: Style.font.menuFamily
                    onClicked: root.startEditing()
                  }

                  Button {
                    text: activateCommand.running ? "Activating…" : "Activate"
                    bordered: true
                    enabled: root.selected !== null && root.selected.enabled && !root.selected.active && !root.busy
                    opacity: enabled ? 1 : 0.4
                    foreground: Color.menu.text
                    fontFamily: Style.font.menuFamily
                    onClicked: root.activate(root.cursor)
                  }
                }
              }

              Column {
                width: parent.width
                spacing: Style.spacing.sm

                PanelSectionHeader {
                  text: "DPI stages"
                  foreground: Color.menu.text
                }

                Row {
                  spacing: Style.spacing.sm

                  Repeater {
                    model: root.selected ? Model.dpiStages(root.selected.profile) : []

                    delegate: BorderSurface {
                      required property var modelData
                      width: chipLabel.implicitWidth + Style.spacing.controlPaddingX * 2
                      height: Style.spacing.controlHeight
                      radius: Style.cornerRadius
                      color: modelData.isDefault ? Style.selectedAccentFill : "transparent"
                      borderSpec: Border.controlSpec(modelData.isDefault ? "focus" : "normal", Color.menu.text, Color.accent)

                      Text {
                        id: chipLabel
                        anchors.centerIn: parent
                        textFormat: Text.PlainText
                        text: modelData.isShift ? modelData.dpi + "  shift" : String(modelData.dpi)
                        color: Color.menu.text
                        font.family: Style.font.menuFamily
                        font.pixelSize: Style.font.body
                        font.bold: modelData.isDefault
                      }
                    }
                  }
                }
              }

              Row {
                id: buttonsArea
                readonly property var views: Model.pictureViews(root.picture)
                width: parent.width
                spacing: Style.spacing.huge

                Row {
                  id: pictures
                  visible: buttonsArea.views.length > 0
                  spacing: Style.spacing.lg

                  Repeater {
                    model: buttonsArea.views

                    delegate: Item {
                      id: view
                      required property var modelData
                      width: Model.viewWidth(modelData, root.pictureHeight)
                      height: root.pictureHeight

                      Image {
                        anchors.fill: parent
                        source: "file://" + view.modelData.image
                        sourceSize.height: root.pictureHeight * 2
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        smooth: true
                        mipmap: true
                      }

                      Repeater {
                        model: view.modelData.hotspots

                        delegate: Rectangle {
                          id: badge
                          required property var modelData
                          readonly property bool hot: root.hoveredSlot === modelData.slot
                          width: Style.space(20)
                          height: width
                          radius: width / 2
                          x: modelData.x * view.width - width / 2
                          y: modelData.y * view.height - height / 2
                          color: hot ? Color.accent : Util.alpha(Color.menu.background, 0.85)
                          border.width: Math.max(1, Style.normalBorderWidth)
                          border.color: hot ? Color.accent : Util.alpha(Color.menu.text, 0.7)

                          Text {
                            anchors.centerIn: parent
                            textFormat: Text.PlainText
                            text: String(badge.modelData.slot)
                            color: badge.hot ? Color.menu.background : Color.menu.text
                            font.family: Style.font.menuFamily
                            font.pixelSize: Style.font.caption
                            font.bold: true
                          }

                          MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onContainsMouseChanged: root.hoverSlot(badge.modelData.slot, containsMouse)
                          }
                        }
                      }
                    }
                  }
                }

                BindingTable {
                  width: parent.width - (pictures.visible ? pictures.width + parent.spacing : 0)
                  rows: root.selected ? Model.bindingRows(root.selected.labels) : []
                  showGShift: root.selected !== null && Model.boundSlots(root.selected.labels.gshift_buttons).length > 0
                }
              }
            }

            // ---- Profile editor ------------------------------------------
            Item {
              id: editor
              anchors.left: divider.right
              anchors.leftMargin: Style.spacing.lg
              anchors.right: parent.right
              anchors.top: parent.top
              anchors.bottom: parent.bottom
              visible: root.editing && root.draft !== null

              Row {
                id: editorHeader
                anchors.left: parent.left
                anchors.top: parent.top
                spacing: Style.spacing.xxl

                Text {
                  anchors.verticalCenter: parent.verticalCenter
                  textFormat: Text.PlainText
                  text: root.draft ? "Edit profile " + root.draft.number : ""
                  color: Color.menu.text
                  font.family: Style.font.menuFamily
                  font.pixelSize: Style.font.heading
                  font.bold: true
                }

                ButtonGroup {
                  anchors.verticalCenter: parent.verticalCenter
                  options: [
                    { value: "dpi", label: "DPI & rate" },
                    { value: "buttons", label: "Buttons" },
                    { value: "gshift", label: "G-Shift" }
                  ]
                  value: root.editTab
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onChanged: function(value) { root.editTab = value }
                }
              }

              Item {
                id: editorBody
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: editorHeader.bottom
                anchors.topMargin: Style.spacing.xxl
                anchors.bottom: previewBox.visible ? previewBox.top : editorActions.top
                anchors.bottomMargin: Style.spacing.lg

                // DPI stages, default and shift stages, report rate.
                Column {
                  anchors.fill: parent
                  visible: root.editTab === "dpi"
                  spacing: Style.spacing.xxl

                  Column {
                    width: parent.width
                    spacing: Style.spacing.sm

                    PanelSectionHeader {
                      text: "DPI stages (" + root.dpiBounds.min + "–" + root.dpiBounds.max + ", step " + root.dpiBounds.step + ")"
                      foreground: Color.menu.text
                    }

                    Flow {
                      width: parent.width
                      spacing: Style.spacing.md

                      Repeater {
                        model: root.draft ? root.draft.dpiStages : []

                        delegate: Row {
                          required property var modelData
                          required property int index
                          spacing: Style.spacing.xxs

                          NumberField {
                            anchors.verticalCenter: parent.verticalCenter
                            from: root.dpiBounds.min
                            to: root.dpiBounds.max
                            stepSize: root.dpiBounds.step
                            value: modelData
                            foreground: Color.menu.text
                            fontFamily: Style.font.menuFamily
                            onModified: function(value) { root.updateDraft(Model.setStage(root.draft, index, value)) }
                          }

                          PanelActionButton {
                            anchors.verticalCenter: parent.verticalCenter
                            visible: root.draft !== null && root.draft.dpiStages.length > 1
                            iconText: "󰅖"
                            tooltipText: "Remove stage"
                            foreground: Color.menu.text
                            onClicked: root.updateDraft(Model.removeStage(root.draft, index))
                          }
                        }
                      }

                      Button {
                        visible: root.draft !== null && root.draft.dpiStages.length < 5
                        text: "Add stage"
                        bordered: true
                        foreground: Color.menu.text
                        fontFamily: Style.font.menuFamily
                        onClicked: root.updateDraft(Model.addStage(root.draft, Model.nextStageDpi(root.draft, root.dpiBounds)))
                      }
                    }
                  }

                  Row {
                    spacing: Style.spacing.huge

                    Dropdown {
                      width: Style.spacing.dropdownWidth
                      label: "Default stage"
                      fontFamily: Style.font.menuFamily
                      options: root.draft ? Model.stageOptions(root.draft) : []
                      value: root.draft && root.draft.defaultDpi !== null ? String(root.draft.defaultDpi) : ""
                      onChanged: function(value) { root.updateDraft(Model.setField(root.draft, "defaultDpi", Number(value))) }
                    }

                    Dropdown {
                      width: Style.spacing.dropdownWidth
                      label: "DPI shift stage"
                      fontFamily: Style.font.menuFamily
                      options: root.draft ? Model.stageOptions(root.draft) : []
                      value: root.draft && root.draft.shiftDpi !== null ? String(root.draft.shiftDpi) : ""
                      onChanged: function(value) { root.updateDraft(Model.setField(root.draft, "shiftDpi", Number(value))) }
                    }
                  }

                  Dropdown {
                    width: Style.spacing.dropdownWidth
                    label: "Report rate"
                    fontFamily: Style.font.menuFamily
                    options: Model.rateOptions(root.info)
                    value: root.draft && root.draft.rateHz !== null ? String(root.draft.rateHz) : ""
                    onChanged: function(value) { root.updateDraft(Model.setField(root.draft, "rateHz", Number(value))) }
                  }
                }

                // Button and G-Shift bindings.
                ListView {
                  anchors.fill: parent
                  visible: root.editTab !== "dpi"
                  clip: true
                  spacing: Style.spacing.sm
                  boundsBehavior: Flickable.StopAtBounds
                  model: root.original ? Model.editableSlots(root.original, root.editTable, root.buttonCount) : []

                  delegate: Row {
                    required property var modelData
                    readonly property int slot: modelData
                    readonly property string table: root.editTable
                    readonly property var current: root.draft ? root.draft[table][slot] : null
                    spacing: Style.spacing.md

                    Caption {
                      anchors.verticalCenter: parent.verticalCenter
                      width: root.slotColumnWidth
                      text: "slot " + slot
                      font.pixelSize: Style.font.body
                    }

                    SearchableDropdown {
                      anchors.verticalCenter: parent.verticalCenter
                      width: Style.spacing.searchableDropdownWidth
                      showLabel: false
                      fontFamily: Style.font.menuFamily
                      placeholderText: "Search actions..."
                      options: Model.actionOptions(root.catalog, current)
                      value: Model.actionChoice(current)
                      onChanged: function(value) {
                        var action = value === "key:" ? (Model.isKeyAction(current) ? current : "key:") : value
                        root.updateDraft(Model.setBinding(root.draft, table, slot, action))
                      }
                    }

                    TextField {
                      anchors.verticalCenter: parent.verticalCenter
                      width: Style.space(170)
                      visible: Model.isKeyAction(current)
                      text: Model.keyCombo(current)
                      placeholderText: "ctrl+shift+t"
                      foreground: Color.menu.text
                      onEditingFinished: root.updateDraft(Model.setBinding(root.draft, table, slot, "key:" + text.trim().toLowerCase()))
                    }
                  }
                }
              }

              BorderSurface {
                id: previewBox
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: editorActions.top
                anchors.bottomMargin: Style.spacing.lg
                visible: root.previewText !== ""
                height: Math.min(previewLabel.implicitHeight + Style.spacing.controlPaddingY * 2, Style.space(150))
                radius: Style.cornerRadius
                color: Util.alpha(Color.menu.text, 0.04)
                borderSpec: Border.controlSpec("normal", Color.menu.text, Color.accent)

                Text {
                  id: previewLabel
                  anchors.fill: parent
                  anchors.margins: Style.spacing.controlPaddingY
                  textFormat: Text.PlainText
                  text: root.previewText
                  color: root.previewIsError ? Color.urgent : Color.menu.text
                  wrapMode: Text.Wrap
                  elide: Text.ElideRight
                  font.family: Style.font.family
                  font.pixelSize: Style.font.body
                }
              }

              Row {
                id: editorActions
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                spacing: Style.spacing.md

                Button {
                  text: previewCommand.running ? "Checking…" : "Preview"
                  bordered: true
                  enabled: !root.busy
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.requestPreview()
                }

                Button {
                  text: saveCommand.running ? "Writing…" : "Save to mouse"
                  bordered: true
                  enabled: root.previewOk && !root.busy
                  opacity: enabled ? 1 : 0.4
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.requestSave()
                }

                Button {
                  text: "Cancel"
                  bordered: true
                  enabled: !saveCommand.running
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.stopEditing()
                }
              }
            }
          }
        }

        Item {
          id: footer
          width: parent.width
          height: Style.font.body + Style.spacing.sm * 2

          Text {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - hints.width - Style.spacing.panelGap
            textFormat: Text.PlainText
            text: root.footerText
            color: root.footerIsError ? Color.urgent : Color.menu.text
            elide: Text.ElideRight
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.body
          }

          Caption {
            id: hints
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            text: root.editing
              ? "Preview, then save    esc cancel"
              : "↑↓ select    ⏎ activate    e edit    r refresh    esc close"
          }
        }
      }

      ConfirmDialog {
        id: confirmDialog
        anchors.fill: parent
        z: 10
        opened: root.confirmOpen
        message: root.draft
          ? "Write profile " + root.draft.number + " to the mouse? A backup of all profiles is saved first, and the write is read back to verify it."
          : ""
        confirmText: "Write"
        fontFamily: Style.font.menuFamily
        onCanceled: {
          root.confirmOpen = false
          keyCatcher.forceActiveFocus()
        }
        onConfirmed: root.confirmSave()
      }
    }
  }
}
