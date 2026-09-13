import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Omalogi's overlay: the connected mouse and its onboard profiles.
// Every device read and write goes through the omalogi CLI, one command at a
// time, so the plugin never touches the device itself.
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

  readonly property var profiles: root.onboard ? root.onboard.profiles : []
  readonly property string footerText: root.notice !== "" ? root.notice : Model.daemonProblem(root.daemon)
  readonly property bool footerIsError: root.notice !== "" ? root.noticeIsError : root.footerText !== ""
  readonly property var selected: root.profiles.length > 0
    ? root.profiles[Model.clampCursor(root.cursor, root.profiles.length)]
    : null
  readonly property bool busy: infoCommand.running || profilesCommand.running || activateCommand.running
  readonly property bool ready: root.info !== null && root.onboard !== null
  readonly property string unreadable: "omalogi answered with output Omalogi could not read."

  readonly property int cardWidth: Math.min(Style.space(880), panel.width - Style.gapsOut * 2)
  readonly property int cardHeight: Math.min(Style.space(600), panel.height - Style.gapsOut * 2)
  readonly property int headerHeight: Math.max(Style.space(40), Style.font.heading + Style.spacing.controlPaddingY * 2)
  readonly property int listWidth: Style.space(240)
  readonly property int slotColumnWidth: Style.space(56)

  function open(payloadJson) {
    exitAnimation.stop()
    root.mounted = true
    root.opened = true
    enterAnimation.restart()
    root.refresh()
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
    if (root.shell && root.manifest) root.shell.hide(root.manifest.id)
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

  function daemonUpdated(state) {
    var previous = root.daemon
    root.daemon = state
    // The daemon switched profiles while the overlay is open: show the new active one.
    var switched = state !== null && (previous === null || previous.active_profile !== state.active_profile)
    if (root.opened && root.ready && switched) profilesCommand.start(["profiles", "--json"])
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

  component BindingList: Column {
    id: list
    property string title: ""
    property var entries: []

    spacing: Style.spacing.xs

    PanelSectionHeader {
      text: list.title
      foreground: Color.menu.text
    }

    Repeater {
      model: list.entries

      delegate: Row {
        required property var modelData
        width: list.width
        spacing: Style.spacing.md

        Caption {
          width: root.slotColumnWidth
          text: "slot " + modelData.slot
          font.pixelSize: Style.font.body
        }

        Text {
          width: parent.width - root.slotColumnWidth - parent.spacing
          textFormat: Text.PlainText
          text: modelData.label
          color: Color.menu.text
          elide: Text.ElideRight
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.body
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

      MouseArea { anchors.fill: parent; onClicked: {} }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.onPressed: function(event) {
          if (event.key === Qt.Key_Escape) {
            root.close()
          } else if (event.key === Qt.Key_Down || event.text === "j") {
            root.moveCursor(1)
          } else if (event.key === Qt.Key_Up || event.text === "k") {
            root.moveCursor(-1)
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
            root.activate(root.cursor)
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

            Column {
              anchors.left: divider.right
              anchors.leftMargin: Style.spacing.lg
              anchors.right: parent.right
              anchors.top: parent.top
              spacing: Style.spacing.xxl
              visible: root.selected !== null

              Item {
                width: parent.width
                height: Math.max(titleColumn.height, activateButton.height)

                Column {
                  id: titleColumn
                  anchors.left: parent.left
                  anchors.right: activateButton.left
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

                Button {
                  id: activateButton
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  text: activateCommand.running ? "Activating…" : "Activate"
                  bordered: true
                  enabled: root.selected !== null && root.selected.enabled && !root.selected.active && !root.busy
                  opacity: enabled ? 1 : 0.4
                  foreground: Color.menu.text
                  fontFamily: Style.font.menuFamily
                  onClicked: root.activate(root.cursor)
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
                width: parent.width
                spacing: Style.spacing.huge

                BindingList {
                  width: (parent.width - parent.spacing) / 2
                  title: "Buttons"
                  entries: root.selected ? Model.boundSlots(root.selected.labels.buttons) : []
                }

                BindingList {
                  width: (parent.width - parent.spacing) / 2
                  title: "G-Shift"
                  entries: root.selected ? Model.boundSlots(root.selected.labels.gshift_buttons) : []
                  visible: entries.length > 0
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
            text: "↑↓ select    ⏎ activate    r refresh    esc close"
          }
        }
      }
    }
  }
}
