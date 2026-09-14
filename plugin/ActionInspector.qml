import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// What the selected button does, and everything it can do instead: the current action,
// a searchable list grouped like G HUB's, and a recorder for keyboard shortcuts.
Item {
  id: inspector

  // The selected button's entry, {slot, name, label, changed, action}, or null.
  property var entry: null
  property var catalog: []
  property bool recording: false
  property string recordingHint: ""
  property string query: ""

  signal chosen(string action)
  signal reverted()

  readonly property int slot: entry ? entry.slot : -1
  readonly property var rows: Model.actionRows(Model.actionSections(catalog, query))

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
        text: inspector.entry && inspector.entry.changed ? "Changed, not on the mouse yet" : "As saved on the mouse"
        font.pixelSize: Style.font.caption
      }
    }

    // The current action.
    Rectangle {
      width: parent.width
      height: Style.spacing.controlHeight + Style.spacing.md * 2
      radius: Style.cornerRadius
      color: Util.alpha(Color.accent, 0.1)
      border.width: Math.max(1, Style.normalBorderWidth)
      border.color: Util.alpha(Color.accent, 0.5)

      Column {
        anchors.left: parent.left
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

        Label {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          text: inspector.recordingHint
          font.pixelSize: Style.font.title
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

  ListView {
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: header.bottom
    anchors.topMargin: Style.spacing.md
    anchors.bottom: parent.bottom
    visible: inspector.entry !== null && !inspector.recording
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    model: inspector.rows

    delegate: Item {
      id: row
      required property var modelData
      readonly property bool isHeader: modelData.kind === "header"
      readonly property bool current: !isHeader && inspector.entry !== null
        && Model.actionChoice(inspector.entry.action) === modelData.value

      width: ListView.view.width
      height: isHeader ? Style.font.caption + Style.spacing.md * 2 : Style.spacing.controlHeight

      Label {
        visible: row.isHeader
        anchors.left: parent.left
        anchors.leftMargin: Style.spacing.md
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Style.spacing.xs
        opacity: 0.55
        text: row.isHeader ? row.modelData.group : ""
        font.pixelSize: Style.font.caption
      }

      Rectangle {
        visible: !row.isHeader
        anchors.fill: parent
        radius: Style.cornerRadius
        color: hover.containsMouse
          ? Util.alpha(Color.menu.text, 0.08)
          : (row.current ? Util.alpha(Color.accent, 0.12) : "transparent")

        Label {
          anchors.left: parent.left
          anchors.right: check.left
          anchors.leftMargin: Style.spacing.md
          anchors.rightMargin: Style.spacing.sm
          anchors.verticalCenter: parent.verticalCenter
          text: row.isHeader ? "" : row.modelData.label
          color: row.current ? Color.accent : Color.menu.text
        }

        Label {
          id: check
          anchors.right: parent.right
          anchors.rightMargin: Style.spacing.md
          anchors.verticalCenter: parent.verticalCenter
          visible: row.current
          text: "󰄬"
          color: Color.accent
          font.family: Style.font.family
        }

        MouseArea {
          id: hover
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: {
            if (row.modelData.value === "key:") inspector.startRecording()
            else inspector.chosen(row.modelData.value)
          }
        }
      }
    }
  }
}
