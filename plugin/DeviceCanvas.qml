import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// The mouse, one view at a time, as G HUB shows it: each button has a label on a thin
// line that runs to a dot on the button. ‹ › switch between the top and side views, the
// switch underneath swaps the default and G-Shift layers, and a selected button opens a
// small menu at its label. Actions dragged from the library drop onto a dot or label.
Item {
  id: canvas

  // Model.pictureViews(picture).
  property var views: []
  property int viewIndex: 0
  // One entry per button of the shown layer: {slot, name, label, changed, action}.
  property var entries: []
  property var catalog: []
  // onboard.factory, for "Use default".
  property var factory: null
  property string table: "buttons"
  property int selectedSlot: -1
  property int hoveredSlot: -1

  signal slotSelected(int slot)
  signal slotHovered(int slot, bool hovered)
  signal viewRequested(int index)
  signal layerRequested(bool gshift)
  signal actionAssigned(int slot, string action)
  signal recordRequested(int slot)
  signal revertRequested(int slot)

  readonly property bool gshift: table === "gshift"
  readonly property bool hasPicture: views.length > 0
  readonly property int currentView: Math.max(0, Math.min(viewIndex, views.length - 1))
  readonly property var shownViews: hasPicture ? [views[currentView]] : []
  readonly property var entryBySlot: Model.indexBySlot(entries)
  readonly property var looseEntries: hasPicture ? Model.entriesWithoutHotspot(entries, views) : entries

  readonly property int labelWidth: Style.space(200)
  readonly property int labelHeight: Style.space(40)
  readonly property int labelGap: Style.spacing.sm
  readonly property int sideGap: Style.space(64)
  readonly property int footerHeight: Style.space(52)
  readonly property int looseHeight: looseEntries.length > 0 ? labelHeight + Style.spacing.lg : 0
  readonly property int pictureHeight: Model.fitPictureHeight(
    shownViews,
    width - 2 * (labelWidth + sideGap) - Style.space(96),
    0,
    Style.space(240),
    Math.max(Style.space(240), height - footerHeight - looseHeight - Style.space(32)))
  readonly property var layout: Model.calloutLayout(shownViews, pictureHeight, 0, labelHeight, labelGap)
  readonly property real picturesX: Math.round((width - layout.picturesWidth) / 2)
  readonly property real leftX: picturesX - sideGap - labelWidth
  readonly property real rightX: picturesX + layout.picturesWidth + sideGap
  readonly property var selectedEntry: entryBySlot[selectedSlot] === undefined ? null : entryBySlot[selectedSlot]
  readonly property string defaultAction: selectedSlot >= 0
    ? (Model.factoryAction({ factory: canvas.factory }, table, selectedSlot) || "")
    : ""

  function hot(slot) {
    return slot === canvas.selectedSlot || slot === canvas.hoveredSlot
  }

  // The entry for `slot`, or an empty one, so a label never reads an undefined entry.
  function entryFor(slot) {
    var entry = canvas.entryBySlot[slot]
    return entry !== undefined ? entry : { slot: slot, name: "", label: "", changed: false, action: null }
  }

  // The label card showing `slot` in the current view: {cardY, side}, or null.
  function cardFor(slot) {
    var left = canvas.layout.left.filter(function(card) { return card.slot === slot })
    if (left.length > 0) return { cardY: left[0].cardY, side: "left" }
    var right = canvas.layout.right.filter(function(card) { return card.slot === slot })
    if (right.length > 0) return { cardY: right[0].cardY, side: "right" }
    return null
  }

  function css(color) {
    return "rgba(" + Math.round(color.r * 255) + "," + Math.round(color.g * 255) + ","
      + Math.round(color.b * 255) + "," + color.a + ")"
  }

  onLayoutChanged: lines.requestPaint()
  onSelectedSlotChanged: lines.requestPaint()
  onHoveredSlotChanged: lines.requestPaint()
  onEntriesChanged: lines.requestPaint()

  component Caption: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.caption
  }

  // A button's label: its name and action above the line that leads to its dot.
  component CalloutLabel: Item {
    id: label
    property var entry: ({ slot: -1, name: "", label: "", changed: false, action: null })
    property bool onLeft: true
    readonly property bool selected: entry.slot === canvas.selectedSlot
    readonly property bool dropping: drop.containsDrag
    readonly property bool hovered: entry.slot === canvas.hoveredSlot || dropping

    width: canvas.labelWidth
    height: canvas.labelHeight

    Column {
      anchors.left: label.onLeft ? undefined : parent.left
      anchors.right: label.onLeft ? parent.right : undefined
      anchors.bottom: parent.bottom
      anchors.bottomMargin: Style.space(4)
      width: parent.width
      spacing: 0

      Caption {
        width: parent.width
        horizontalAlignment: label.onLeft ? Text.AlignRight : Text.AlignLeft
        opacity: 0.5
        text: label.entry.name
      }

      Row {
        anchors.right: label.onLeft ? parent.right : undefined
        layoutDirection: label.onLeft ? Qt.RightToLeft : Qt.LeftToRight
        spacing: Style.spacing.xs

        Icon {
          anchors.verticalCenter: parent.verticalCenter
          name: Model.actionIcon(label.entry.action)
          tint: label.selected || label.entry.changed || label.dropping ? Color.accent : Color.menu.text
          size: Style.space(15)
        }

        Text {
          anchors.verticalCenter: parent.verticalCenter
          width: Math.min(implicitWidth, canvas.labelWidth - Style.space(22))
          textFormat: Text.PlainText
          text: label.entry.label
          elide: Text.ElideRight
          color: label.selected || label.entry.changed || label.dropping ? Color.accent : Color.menu.text
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.body
          font.bold: label.selected
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: canvas.slotSelected(label.entry.slot === canvas.selectedSlot ? -1 : label.entry.slot)
      onContainsMouseChanged: canvas.slotHovered(label.entry.slot, containsMouse)
    }

    DropArea {
      id: drop
      anchors.fill: parent
      keys: ["omalogi-action"]
      onDropped: function(event) { canvas.actionAssigned(label.entry.slot, event.source.action) }
    }
  }

  // Clicking empty space clears the selection.
  MouseArea {
    anchors.fill: parent
    onClicked: canvas.slotSelected(-1)
  }

  Item {
    id: content
    width: canvas.width
    height: canvas.layout.height
    y: Math.max(0, Math.round((canvas.height - canvas.footerHeight - canvas.looseHeight - canvas.layout.height) / 2))
    visible: canvas.hasPicture

    Canvas {
      id: lines
      anchors.fill: parent

      onPaint: {
        var ctx = getContext("2d")
        ctx.reset()
        var dim = canvas.css(Util.alpha(Color.menu.text, 0.28))
        var accent = canvas.css(Color.accent)
        function draw(card, onLeft) {
          if (canvas.entryBySlot[card.slot] === undefined) return
          var hot = canvas.hot(card.slot)
          var y = card.cardY + canvas.labelHeight
          var outer = onLeft ? canvas.leftX : canvas.rightX + canvas.labelWidth
          var inner = onLeft ? canvas.leftX + canvas.labelWidth : canvas.rightX
          ctx.beginPath()
          ctx.strokeStyle = hot ? accent : dim
          ctx.lineWidth = hot ? 2 : 1
          ctx.moveTo(outer, y)
          ctx.lineTo(inner, y)
          ctx.lineTo(canvas.picturesX + card.x, card.y)
          ctx.stroke()
        }
        canvas.layout.left.forEach(function(card) { draw(card, true) })
        canvas.layout.right.forEach(function(card) { draw(card, false) })
      }
    }

    Image {
      x: canvas.picturesX
      width: canvas.layout.picturesWidth
      height: canvas.pictureHeight
      source: canvas.shownViews.length > 0 ? "file://" + canvas.shownViews[0].image : ""
      // Decoded once at a fixed size; resizing only rescales.
      sourceSize.height: 1024
      fillMode: Image.PreserveAspectFit
      asynchronous: true
      smooth: true
      mipmap: true
    }

    Repeater {
      model: canvas.layout.left.concat(canvas.layout.right)

      delegate: Item {
        id: dot
        required property var modelData
        readonly property bool hot: canvas.hot(modelData.slot) || dotDrop.containsDrag
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        width: Style.space(34)
        height: width
        x: canvas.picturesX + modelData.x - width / 2
        y: modelData.y - height / 2

        Rectangle {
          anchors.centerIn: parent
          width: dotDrop.containsDrag ? Style.space(20) : (dot.hot ? Style.space(15) : Style.space(11))
          height: width
          radius: width / 2
          color: dot.hot ? Color.accent : Color.menu.text
          border.width: Math.max(2, Style.normalBorderWidth)
          border.color: Color.menu.background

          Behavior on width { NumberAnimation { duration: 90 } }
        }

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: canvas.slotSelected(dot.modelData.slot === canvas.selectedSlot ? -1 : dot.modelData.slot)
          onContainsMouseChanged: canvas.slotHovered(dot.modelData.slot, containsMouse)
        }

        DropArea {
          id: dotDrop
          anchors.fill: parent
          keys: ["omalogi-action"]
          onDropped: function(event) { canvas.actionAssigned(dot.modelData.slot, event.source.action) }
        }
      }
    }

    Repeater {
      model: canvas.layout.left

      delegate: CalloutLabel {
        required property var modelData
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        entry: canvas.entryFor(modelData.slot)
        onLeft: true
        x: canvas.leftX
        y: modelData.cardY
      }
    }

    Repeater {
      model: canvas.layout.right

      delegate: CalloutLabel {
        required property var modelData
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        entry: canvas.entryFor(modelData.slot)
        onLeft: false
        x: canvas.rightX
        y: modelData.cardY
      }
    }
  }

  // ‹ › between the top and side views.
  Caption {
    anchors.horizontalCenter: parent.horizontalCenter
    y: Math.max(0, content.y - Style.space(28))
    visible: canvas.views.length > 1
    opacity: 0.5
    font.letterSpacing: 1.5
    text: canvas.shownViews.length > 0 ? (canvas.shownViews[0].name === "side" ? "SIDE" : "TOP") : ""
  }

  PanelActionButton {
    anchors.left: parent.left
    y: content.y + canvas.layout.height / 2 - height / 2
    visible: canvas.views.length > 1
    iconText: "󰅁"
    tooltipText: "Previous view"
    foreground: Color.menu.text
    onClicked: canvas.viewRequested((canvas.currentView + canvas.views.length - 1) % canvas.views.length)
  }

  PanelActionButton {
    anchors.right: parent.right
    y: content.y + canvas.layout.height / 2 - height / 2
    visible: canvas.views.length > 1
    iconText: "󰅂"
    tooltipText: "Next view"
    foreground: Color.menu.text
    onClicked: canvas.viewRequested((canvas.currentView + 1) % canvas.views.length)
  }

  // Buttons the pictures do not show, such as extra wheel bindings, or every button
  // without a picture.
  Flow {
    anchors.horizontalCenter: parent.horizontalCenter
    y: canvas.hasPicture ? content.y + canvas.layout.height + Style.spacing.lg : 0
    width: Math.min(parent.width, (canvas.labelWidth + Style.spacing.lg) * Math.max(1, canvas.looseEntries.length))
    spacing: Style.spacing.lg

    Repeater {
      model: canvas.looseEntries

      delegate: CalloutLabel {
        required property var modelData
        entry: modelData
        onLeft: false
      }
    }
  }

  // DEFAULT ◯ G-SHIFT, as under G HUB's device.
  Row {
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: Style.spacing.md
    spacing: Style.spacing.md

    Caption {
      anchors.verticalCenter: parent.verticalCenter
      font.letterSpacing: 1.5
      font.bold: !canvas.gshift
      opacity: canvas.gshift ? 0.5 : 1
      color: canvas.gshift ? Color.menu.text : Color.accent
      text: "DEFAULT"

      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: canvas.layerRequested(false)
      }
    }

    ToggleSwitch {
      anchors.verticalCenter: parent.verticalCenter
      checked: canvas.gshift
      foreground: Color.menu.text
      accent: Color.accent
      onToggled: canvas.layerRequested(!canvas.gshift)
    }

    Caption {
      anchors.verticalCenter: parent.verticalCenter
      font.letterSpacing: 1.5
      font.bold: canvas.gshift
      opacity: canvas.gshift ? 1 : 0.5
      color: canvas.gshift ? Color.accent : Color.menu.text
      text: "G-SHIFT"

      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: canvas.layerRequested(true)
      }
    }
  }

  // The selected button's menu, at its label.
  Rectangle {
    id: menu
    readonly property var card: canvas.selectedEntry ? canvas.cardFor(canvas.selectedSlot) : null
    visible: canvas.selectedEntry !== null
    z: 20
    width: Style.space(250)
    height: menuColumn.implicitHeight + Style.spacing.sm * 2
    x: card === null
      ? (canvas.width - width) / 2
      : (card.side === "left" ? canvas.leftX : canvas.rightX + canvas.labelWidth - width)
    y: {
      var below = card === null
        ? canvas.height - canvas.footerHeight - height - Style.spacing.md
        : content.y + card.cardY + canvas.labelHeight + Style.spacing.sm
      return Math.max(0, Math.min(below, canvas.height - canvas.footerHeight - height))
    }
    radius: Style.cornerRadius
    color: Color.menu.background
    border.width: Math.max(1, Style.normalBorderWidth)
    border.color: Util.alpha(Color.accent, 0.7)

    // Keeps clicks inside the menu from clearing the selection.
    MouseArea { anchors.fill: parent }

    component MenuItem: Rectangle {
      id: item
      property string icon: ""
      property string text: ""
      property string detail: ""
      signal activated()

      width: menuColumn.width
      height: Style.space(34)
      radius: Style.cornerRadius
      opacity: enabled ? 1 : 0.4
      color: itemArea.containsMouse && enabled ? Util.alpha(Color.menu.text, 0.08) : "transparent"

      Icon {
        id: itemIcon
        anchors.left: parent.left
        anchors.leftMargin: Style.spacing.sm
        anchors.verticalCenter: parent.verticalCenter
        name: item.icon
        size: Style.space(16)
      }

      Text {
        anchors.left: itemIcon.right
        anchors.right: parent.right
        anchors.leftMargin: Style.spacing.sm
        anchors.rightMargin: Style.spacing.sm
        anchors.verticalCenter: parent.verticalCenter
        textFormat: Text.PlainText
        elide: Text.ElideRight
        text: item.detail === "" ? item.text : item.text + "  ·  " + item.detail
        color: Color.menu.text
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.bodySmall
      }

      MouseArea {
        id: itemArea
        anchors.fill: parent
        hoverEnabled: true
        enabled: item.enabled
        cursorShape: Qt.PointingHandCursor
        onClicked: item.activated()
      }
    }

    Column {
      id: menuColumn
      anchors.fill: parent
      anchors.margins: Style.spacing.sm
      spacing: Style.space(2)

      Row {
        spacing: Style.spacing.sm
        leftPadding: Style.spacing.sm
        bottomPadding: Style.spacing.xs

        Icon {
          anchors.verticalCenter: parent.verticalCenter
          name: canvas.selectedEntry ? Model.actionIcon(canvas.selectedEntry.action) : ""
          tint: Color.accent
          size: Style.space(18)
        }

        Text {
          anchors.verticalCenter: parent.verticalCenter
          width: menuColumn.width - Style.space(40)
          textFormat: Text.PlainText
          elide: Text.ElideRight
          text: canvas.selectedEntry ? canvas.selectedEntry.name + "  ·  " + canvas.selectedEntry.label : ""
          color: Color.menu.text
          font.family: Style.font.menuFamily
          font.pixelSize: Style.font.body
          font.bold: true
        }
      }

      MenuItem {
        icon: "rotate-ccw"
        text: "Use default"
        detail: canvas.defaultAction !== "" ? Model.actionLabel(canvas.catalog, canvas.defaultAction, "", true) : ""
        enabled: canvas.selectedEntry !== null && canvas.defaultAction !== ""
          && canvas.defaultAction !== canvas.selectedEntry.action
        onActivated: canvas.actionAssigned(canvas.selectedSlot, canvas.defaultAction)
      }

      MenuItem {
        icon: "ban"
        text: "Disable"
        enabled: canvas.selectedEntry !== null && canvas.selectedEntry.action !== "disabled"
        onActivated: canvas.actionAssigned(canvas.selectedSlot, "disabled")
      }

      MenuItem {
        icon: "keyboard"
        text: "Record a shortcut…"
        onActivated: canvas.recordRequested(canvas.selectedSlot)
      }

      MenuItem {
        icon: "undo-2"
        text: "Undo this change"
        visible: canvas.selectedEntry !== null && canvas.selectedEntry.changed
        onActivated: canvas.revertRequested(canvas.selectedSlot)
      }
    }
  }
}
