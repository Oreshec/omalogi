import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// The action library beside the mouse, as in G HUB's assignments panel: category tabs,
// search, and a list of actions to drag onto a button or pick for the selected one.
// Keyboard shortcuts are recorded from the keys the user presses.
Item {
  id: library

  property var catalog: []
  // Entries of the shown layer, for the badges naming the buttons that use an action.
  property var entries: []
  // The selected button's entry, or null.
  property var entry: null
  property string layerName: "Default layer"
  // Whether the G-Shift layer is shown, and the buttons that reach it, e.g. ["G5"].
  property bool gshift: false
  property var gshiftButtons: []
  readonly property bool unreachable: gshift && gshiftButtons.length === 0
  // The item that follows the pointer while an action is dragged (see Omalogi.qml).
  property Item dragProxy: null
  property string group: "Mouse"
  property string query: ""
  property bool recording: false
  property string recordingHint: ""
  property int recordingSlot: -1

  signal chosen(string action)
  signal shortcutRecorded(int slot, string action)
  signal selectionNeeded()

  readonly property bool searching: query.trim() !== ""
  readonly property var groups: Model.actionGroups(catalog)
  readonly property var rows: searching
    ? Model.actionRows(Model.actionSections(catalog, query))
    : Model.actionRows(Model.actionSections(catalog, "").filter(function(section) {
        return section.group === library.group
      })).filter(function(row) { return row.kind === "action" })

  function startRecording(slot) {
    library.recordingSlot = slot
    library.recordingHint = "Press the shortcut"
    library.recording = true
    recorder.forceActiveFocus()
  }

  function stopRecording() {
    library.recording = false
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.body
  }

  Column {
    id: header
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    spacing: Style.spacing.md

    Column {
      width: parent.width
      spacing: Style.spacing.xxs

      Label {
        width: parent.width
        text: "Assignments"
        font.pixelSize: Style.font.heading
        font.bold: true
      }

      Label {
        width: parent.width
        opacity: 0.6
        text: library.gshift && library.gshiftButtons.length > 0
          ? library.layerName + " · hold " + library.gshiftButtons.join(" or ")
          : library.layerName
        font.pixelSize: Style.font.caption
      }
    }

    Label {
      width: parent.width
      wrapMode: Text.Wrap
      opacity: library.unreachable ? 1 : 0.7
      color: library.unreachable ? Color.accent : Color.menu.text
      text: {
        if (library.unreachable)
          return "No button holds G-Shift on this profile, so these actions can't be used yet. Give a button the G-Shift action on the Default layer first."
        return library.entry
          ? "Pick an action for " + library.entry.name + ", or drag one onto any button."
          : "Drag an action onto a button, or select a button and pick one."
      }
    }

    TextField {
      id: search
      width: parent.width
      placeholderText: "Search actions"
      foreground: Color.menu.text
      onTextChanged: library.query = text
      Keys.onEscapePressed: {
        text = ""
        focus = false
      }
    }

    // Category tabs, underlined like G HUB's, on one line.
    Flow {
      width: parent.width
      spacing: Style.spacing.sm
      visible: !library.searching

      Repeater {
        model: library.groups

        delegate: Item {
          id: tab
          required property string modelData
          readonly property bool selected: library.group === modelData
          width: tabText.implicitWidth
          height: tabText.implicitHeight + Style.space(8)

          Text {
            id: tabText
            textFormat: Text.PlainText
            text: tab.modelData.toUpperCase()
            color: tab.selected ? Color.menu.text : Color.menu.text
            opacity: tab.selected ? 1 : (tabArea.containsMouse ? 0.8 : 0.5)
            font.family: Style.font.menuFamily
            font.pixelSize: Style.font.caption
            font.letterSpacing: 0.6
            font.bold: tab.selected
          }

          Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: Math.max(2, Style.space(2))
            visible: tab.selected
            color: Color.accent
          }

          MouseArea {
            id: tabArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: library.group = tab.modelData
          }
        }
      }
    }
  }

  // Shortcut recorder.
  Rectangle {
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: header.bottom
    anchors.topMargin: Style.spacing.md
    height: Style.space(120)
    visible: library.recording
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
          text: library.recordingHint
          font.pixelSize: Style.font.title
        }
      }

      Label {
        width: parent.width
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        opacity: 0.6
        text: "Hold any of Ctrl, Shift, Alt or Super, then press a key. Esc cancels."
        font.pixelSize: Style.font.caption
      }
    }

    MouseArea {
      anchors.fill: parent
      onClicked: recorder.forceActiveFocus()
    }
  }

  Item {
    id: recorder
    width: 0
    height: 0

    Keys.onPressed: function(event) {
      if (!library.recording) return
      event.accepted = true
      if (event.key === Qt.Key_Escape && (event.modifiers & (Qt.ControlModifier | Qt.ShiftModifier | Qt.AltModifier | Qt.MetaModifier)) === 0) {
        library.stopRecording()
        return
      }
      var result = Model.recordKey(event.nativeScanCode, event.modifiers)
      if (result.waiting) {
        library.recordingHint = Model.modifiersLabel(event.modifiers) + "+…"
        return
      }
      if (result.unsupported) {
        library.recordingHint = "The mouse can't send that key"
        return
      }
      library.stopRecording()
      library.shortcutRecorded(library.recordingSlot, "key:" + result.combo)
    }
  }

  ListView {
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: header.bottom
    anchors.topMargin: Style.spacing.md
    anchors.bottom: parent.bottom
    visible: !library.recording
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    model: library.rows

    Label {
      anchors.centerIn: parent
      visible: library.rows.length === 0
      opacity: 0.6
      text: "No actions match"
    }

    delegate: Item {
      id: row
      required property var modelData
      readonly property bool isAction: modelData.kind === "action"
      readonly property bool isShortcut: isAction && modelData.value === "key:"
      readonly property bool current: isAction && library.entry !== null
        && Model.actionChoice(library.entry.action) === modelData.value
      readonly property var assigned: isAction && !isShortcut ? Model.assignedNames(library.entries, modelData.value) : []

      width: ListView.view.width
      height: isAction ? Style.space(40) : Style.space(30)

      Label {
        visible: !row.isAction
        anchors.left: parent.left
        anchors.leftMargin: Style.spacing.sm
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Style.spacing.xs
        opacity: 0.5
        text: row.isAction ? "" : row.modelData.group.toUpperCase()
        font.pixelSize: Style.font.caption
        font.letterSpacing: 1.2
      }

      Rectangle {
        visible: row.isAction
        anchors.fill: parent
        anchors.bottomMargin: Style.space(2)
        radius: Style.cornerRadius
        color: row.current
          ? Util.alpha(Color.accent, 0.15)
          : (area.containsMouse ? Util.alpha(Color.menu.text, 0.08) : "transparent")
        border.width: row.current ? Math.max(1, Style.normalBorderWidth) : 0
        border.color: Util.alpha(Color.accent, 0.6)

        Icon {
          id: rowIcon
          anchors.left: parent.left
          anchors.leftMargin: Style.spacing.sm
          anchors.verticalCenter: parent.verticalCenter
          name: row.isAction ? Model.actionIcon(row.modelData.value) : ""
          tint: row.current ? Color.accent : Color.menu.text
          size: Style.space(18)
        }

        Label {
          anchors.left: rowIcon.right
          anchors.right: badges.left
          anchors.leftMargin: Style.spacing.sm
          anchors.rightMargin: Style.spacing.sm
          anchors.verticalCenter: parent.verticalCenter
          text: row.isShortcut ? "Record a shortcut…" : (row.isAction ? row.modelData.label : "")
          color: row.current ? Color.accent : Color.menu.text
        }

        Label {
          id: badges
          anchors.right: parent.right
          anchors.rightMargin: Style.spacing.sm
          anchors.verticalCenter: parent.verticalCenter
          opacity: 0.55
          text: row.assigned.join(" ")
          font.pixelSize: Style.font.caption
        }
      }

      MouseArea {
        id: area
        anchors.fill: parent
        enabled: row.isAction
        hoverEnabled: true
        cursorShape: drag.active ? Qt.ClosedHandCursor : Qt.PointingHandCursor
        drag.target: row.isAction && !row.isShortcut ? library.dragProxy : null
        drag.threshold: Style.space(6)

        onPressed: function(mouse) {
          if (!library.dragProxy || row.isShortcut) return
          var point = mapToItem(library.dragProxy.parent, mouse.x, mouse.y)
          library.dragProxy.prepare(row.modelData, point.x, point.y)
        }

        drag.onActiveChanged: {
          if (!library.dragProxy) return
          if (drag.active) {
            library.dragProxy.dragging = true
          } else {
            library.dragProxy.Drag.drop()
            library.dragProxy.dragging = false
          }
        }

        onClicked: {
          if (!library.entry) {
            library.selectionNeeded()
          } else if (row.isShortcut) {
            library.startRecording(library.entry.slot)
          } else {
            library.chosen(row.modelData.value)
          }
        }
      }
    }
  }
}
