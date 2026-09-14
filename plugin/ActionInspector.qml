import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// What the selected button does and what it can do instead, laid out like G HUB's
// assignments: the current action, category tabs with icons, and a grid of action
// tiles. Keyboard shortcuts are recorded from the keys the user presses.
Item {
  id: inspector

  // The selected button's entry, {slot, name, label, changed, action}, or null.
  property var entry: null
  property var catalog: []
  property bool recording: false
  property string recordingHint: ""
  property string query: ""
  // "All" or one of the catalog's groups.
  property string group: "All"

  signal chosen(string action)
  signal reverted()

  readonly property int slot: entry ? entry.slot : -1
  readonly property bool searching: query.trim() !== ""
  readonly property var groups: ["All"].concat(Model.actionGroups(catalog))
  readonly property var sections: Model.actionSections(catalog, query).filter(function(section) {
    return inspector.searching || inspector.group === "All" || section.group === inspector.group
  })
  readonly property int tileColumns: 2
  readonly property real tileWidth: (width - Style.spacing.sm * (tileColumns - 1)) / tileColumns

  function startRecording() {
    inspector.recordingHint = "Press the shortcut"
    inspector.recording = true
    recorder.forceActiveFocus()
  }

  function stopRecording() {
    inspector.recording = false
  }

  onSlotChanged: {
    stopRecording()
    search.text = ""
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.body
  }

  // Nothing selected.
  Column {
    anchors.centerIn: parent
    width: parent.width
    spacing: Style.spacing.md
    visible: inspector.entry === null

    Icon {
      anchors.horizontalCenter: parent.horizontalCenter
      name: "mouse-pointer-click"
      size: Style.space(40)
      opacity: 0.5
    }

    Label {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: "Select a button"
      font.pixelSize: Style.font.title
    }

    Label {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.Wrap
      opacity: 0.6
      text: "Click a button on the mouse or its card to change what it does."
    }
  }

  Column {
    id: header
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    spacing: Style.spacing.md
    visible: inspector.entry !== null

    Column {
      width: parent.width
      spacing: Style.spacing.xxs

      Label {
        width: parent.width
        text: inspector.entry ? inspector.entry.name : ""
        font.pixelSize: Style.font.heading
        font.bold: true
      }

      Label {
        width: parent.width
        opacity: 0.6
        text: inspector.entry && inspector.entry.changed ? "Changed, saving to the mouse" : "As saved on the mouse"
        font.pixelSize: Style.font.caption
      }
    }

    // The current action.
    Rectangle {
      width: parent.width
      height: Style.space(56)
      radius: Style.cornerRadius
      color: Util.alpha(Color.accent, 0.1)
      border.width: Math.max(1, Style.normalBorderWidth)
      border.color: Util.alpha(Color.accent, 0.5)

      Icon {
        id: currentIcon
        anchors.left: parent.left
        anchors.leftMargin: Style.spacing.md
        anchors.verticalCenter: parent.verticalCenter
        name: inspector.entry ? Model.actionIcon(inspector.entry.action) : ""
        tint: Color.accent
        size: Style.space(24)
      }

      Column {
        anchors.left: currentIcon.right
        anchors.right: revert.left
        anchors.leftMargin: Style.spacing.md
        anchors.rightMargin: Style.spacing.sm
        anchors.verticalCenter: parent.verticalCenter

        Label {
          width: parent.width
          opacity: 0.6
          text: "Current action"
          font.pixelSize: Style.font.caption
        }

        Label {
          width: parent.width
          text: inspector.entry ? inspector.entry.label : ""
        }
      }

      PanelActionButton {
        id: revert
        anchors.right: parent.right
        anchors.rightMargin: Style.spacing.sm
        anchors.verticalCenter: parent.verticalCenter
        visible: inspector.entry !== null && inspector.entry.changed
        iconText: "󰕌"
        tooltipText: "Undo this change"
        foreground: Color.menu.text
        onClicked: inspector.reverted()
      }
    }

    // Shortcut recorder.
    Rectangle {
      width: parent.width
      height: Style.spacing.controlHeight * 2 + Style.spacing.md * 2
      visible: inspector.recording
      radius: Style.cornerRadius
      color: Util.alpha(Color.menu.text, 0.04)
      border.width: Math.max(2, Style.normalBorderWidth)
      border.color: Color.accent

      Column {
        anchors.centerIn: parent
        width: parent.width - Style.spacing.md * 2
        spacing: Style.spacing.sm

        Row {
          anchors.horizontalCenter: parent.horizontalCenter
          spacing: Style.spacing.sm

          Icon {
            anchors.verticalCenter: parent.verticalCenter
            name: "keyboard"
            tint: Color.accent
            size: Style.space(22)
          }

          Label {
            anchors.verticalCenter: parent.verticalCenter
            text: inspector.recordingHint
            font.pixelSize: Style.font.title
          }
        }

        Label {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          opacity: 0.6
          text: "Hold any of Ctrl, Shift, Alt or Super, then press a key. Esc cancels."
          wrapMode: Text.Wrap
          font.pixelSize: Style.font.caption
        }
      }

      MouseArea {
        anchors.fill: parent
        onClicked: recorder.forceActiveFocus()
      }
    }

    TextField {
      id: search
      width: parent.width
      visible: !inspector.recording
      placeholderText: "Search actions"
      foreground: Color.menu.text
      onTextChanged: inspector.query = text
    }

    // Category tabs.
    Row {
      spacing: Style.spacing.xs
      visible: !inspector.recording && !inspector.searching

      Repeater {
        model: inspector.groups

        delegate: Rectangle {
          id: tab
          required property string modelData
          readonly property bool selected: inspector.group === modelData
          width: Math.floor((header.width - Style.spacing.xs * (inspector.groups.length - 1)) / inspector.groups.length)
          height: Style.space(34)
          radius: Style.cornerRadius
          color: selected ? Util.alpha(Color.accent, 0.18) : (tabArea.containsMouse ? Util.alpha(Color.menu.text, 0.08) : "transparent")
          border.width: Math.max(1, Style.normalBorderWidth)
          border.color: selected ? Color.accent : Util.alpha(Color.menu.text, 0.14)

          Icon {
            anchors.centerIn: parent
            name: tab.modelData === "All" ? "layout-grid" : Model.groupIcon(tab.modelData)
            tint: tab.selected ? Color.accent : Color.menu.text
            size: Style.space(18)
          }

          MouseArea {
            id: tabArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: inspector.group = tab.modelData
          }

          PanelToolTip {
            visible: tabArea.containsMouse
            text: tab.modelData
          }
        }
      }
    }
  }

  Item {
    id: recorder
    width: 0
    height: 0

    Keys.onPressed: function(event) {
      if (!inspector.recording) return
      event.accepted = true
      if (event.key === Qt.Key_Escape && (event.modifiers & (Qt.ControlModifier | Qt.ShiftModifier | Qt.AltModifier | Qt.MetaModifier)) === 0) {
        inspector.stopRecording()
        return
      }
      var result = Model.recordKey(event.nativeScanCode, event.modifiers)
      if (result.waiting) {
        inspector.recordingHint = Model.modifiersLabel(event.modifiers) + "+…"
        return
      }
      if (result.unsupported) {
        inspector.recordingHint = "The mouse can't send that key"
        return
      }
      inspector.stopRecording()
      inspector.chosen("key:" + result.combo)
    }
  }

  Flickable {
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: header.bottom
    anchors.topMargin: Style.spacing.md
    anchors.bottom: parent.bottom
    visible: inspector.entry !== null && !inspector.recording
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    contentWidth: width
    contentHeight: tiles.height

    Column {
      id: tiles
      width: parent.width
      spacing: Style.spacing.md

      Label {
        visible: inspector.sections.length === 0
        width: parent.width
        horizontalAlignment: Text.AlignHCenter
        opacity: 0.6
        text: "No actions match"
      }

      Repeater {
        model: inspector.sections

        delegate: Column {
          id: section
          required property var modelData
          width: tiles.width
          spacing: Style.spacing.xs

          Label {
            visible: inspector.group === "All" || inspector.searching
            opacity: 0.55
            text: section.modelData.group
            font.pixelSize: Style.font.caption
          }

          Flow {
            width: parent.width
            spacing: Style.spacing.sm

            Repeater {
              model: section.modelData.actions

              delegate: Rectangle {
                id: tile
                required property var modelData
                readonly property bool current: inspector.entry !== null
                  && Model.actionChoice(inspector.entry.action) === modelData.value
                width: inspector.tileWidth
                height: Style.space(52)
                radius: Style.cornerRadius
                color: current
                  ? Util.alpha(Color.accent, 0.16)
                  : Util.alpha(Color.menu.text, tileArea.containsMouse ? 0.09 : 0.035)
                border.width: current ? Math.max(2, Style.normalBorderWidth) : Math.max(1, Style.normalBorderWidth)
                border.color: current ? Color.accent : Util.alpha(Color.menu.text, tileArea.containsMouse ? 0.35 : 0.12)

                Icon {
                  id: tileIcon
                  anchors.left: parent.left
                  anchors.leftMargin: Style.spacing.md
                  anchors.verticalCenter: parent.verticalCenter
                  name: Model.actionIcon(tile.modelData.value)
                  tint: tile.current ? Color.accent : Color.menu.text
                  size: Style.space(20)
                }

                Label {
                  anchors.left: tileIcon.right
                  anchors.right: parent.right
                  anchors.leftMargin: Style.spacing.sm
                  anchors.rightMargin: Style.spacing.sm
                  anchors.verticalCenter: parent.verticalCenter
                  wrapMode: Text.Wrap
                  maximumLineCount: 2
                  text: tile.modelData.value === "key:" ? "Record a shortcut…" : tile.modelData.label
                  color: tile.current ? Color.accent : Color.menu.text
                  font.pixelSize: Style.font.bodySmall
                }

                MouseArea {
                  id: tileArea
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: {
                    if (tile.modelData.value === "key:") inspector.startRecording()
                    else inspector.chosen(tile.modelData.value)
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
