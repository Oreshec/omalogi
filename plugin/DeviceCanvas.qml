import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// The mouse with a card for each button and a leader line from the card to the button,
// as G HUB and OpenLogi draw it. Buttons without a position on the picture, and every
// button when there is no picture, get their cards in a row underneath.
Item {
  id: canvas

  // Model.pictureViews(picture).
  property var views: []
  // One entry per button of the shown table: {slot, name, label, changed, action}.
  property var entries: []
  property int selectedSlot: -1
  property int hoveredSlot: -1

  signal slotSelected(int slot)
  signal slotHovered(int slot, bool hovered)

  readonly property bool hasPicture: views.length > 0
  readonly property int cardWidth: Style.space(172)
  readonly property int cardHeight: Style.space(46)
  readonly property int cardGap: Style.spacing.sm
  readonly property int sideGap: Style.space(40)
  readonly property int viewGap: Style.spacing.lg
  readonly property var entryBySlot: Model.indexBySlot(entries)
  readonly property var looseEntries: hasPicture ? Model.entriesWithoutHotspot(entries, views) : entries
  readonly property int looseHeight: looseEntries.length > 0 ? cardHeight * 2 + cardGap + Style.spacing.lg : 0
  readonly property int pictureHeight: Model.fitPictureHeight(
    views,
    width - 2 * (cardWidth + sideGap),
    viewGap,
    Style.space(220),
    Math.max(Style.space(220), height - looseHeight))
  readonly property var layout: Model.calloutLayout(views, pictureHeight, viewGap, cardHeight, cardGap)
  readonly property int pictureBlockHeight: hasPicture ? layout.height : 0
  readonly property real picturesX: Math.round((width - layout.picturesWidth) / 2)
  readonly property real leftCardsX: picturesX - sideGap - cardWidth
  readonly property real rightCardsX: picturesX + layout.picturesWidth + sideGap

  function hot(slot) {
    return slot === canvas.selectedSlot || slot === canvas.hoveredSlot
  }

  function entryFor(slot) {
    var entry = canvas.entryBySlot[slot]
    return entry !== undefined ? entry : { slot: slot, name: "", label: "", changed: false, action: null }
  }

  // Context2D wants CSS colors.
  function css(color) {
    return "rgba(" + Math.round(color.r * 255) + "," + Math.round(color.g * 255) + ","
      + Math.round(color.b * 255) + "," + color.a + ")"
  }

  onLayoutChanged: lines.requestPaint()
  onSelectedSlotChanged: lines.requestPaint()
  onHoveredSlotChanged: lines.requestPaint()
  onEntriesChanged: lines.requestPaint()

  component SlotCard: Rectangle {
    id: card
    property var entry: ({ slot: -1, name: "", label: "", changed: false })
    readonly property bool selected: entry.slot === canvas.selectedSlot
    readonly property bool hovered: entry.slot === canvas.hoveredSlot

    width: canvas.cardWidth
    height: canvas.cardHeight
    radius: Style.cornerRadius
    color: selected
      ? Util.alpha(Color.accent, 0.14)
      : Util.alpha(Color.menu.text, hovered ? 0.08 : 0.03)
    border.width: selected ? Math.max(2, Style.normalBorderWidth) : Math.max(1, Style.normalBorderWidth)
    border.color: selected ? Color.accent : Util.alpha(Color.menu.text, hovered ? 0.45 : 0.16)

    Icon {
      id: cardIcon
      anchors.left: parent.left
      anchors.leftMargin: Style.spacing.md
      anchors.verticalCenter: parent.verticalCenter
      name: Model.actionIcon(card.entry.action)
      tint: card.entry.changed || card.selected ? Color.accent : Color.menu.text
      size: Style.space(18)
      opacity: card.entry.name === "" ? 0 : 1
    }

    Column {
      anchors.verticalCenter: parent.verticalCenter
      anchors.left: cardIcon.right
      anchors.right: parent.right
      anchors.leftMargin: Style.spacing.sm
      anchors.rightMargin: Style.spacing.md + Style.space(8)

      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: card.entry.name
        color: Color.menu.text
        opacity: 0.55
        elide: Text.ElideRight
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.caption
      }

      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: card.entry.label
        color: card.entry.changed ? Color.accent : Color.menu.text
        elide: Text.ElideRight
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.body
      }
    }

    // Changed and not written to the mouse yet.
    Rectangle {
      visible: card.entry.changed
      anchors.top: parent.top
      anchors.right: parent.right
      anchors.margins: Style.spacing.sm
      width: Style.space(7)
      height: width
      radius: width / 2
      color: Color.accent
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: canvas.slotSelected(card.entry.slot)
      onContainsMouseChanged: canvas.slotHovered(card.entry.slot, containsMouse)
    }
  }

  Item {
    id: content
    width: canvas.width
    height: canvas.pictureBlockHeight
    y: Math.max(0, Math.round((canvas.height - canvas.pictureBlockHeight - canvas.looseHeight) / 2))
    visible: canvas.hasPicture

    Canvas {
      id: lines
      anchors.fill: parent

      onPaint: {
        var ctx = getContext("2d")
        ctx.reset()
        var dim = canvas.css(Util.alpha(Color.menu.text, 0.3))
        var accent = canvas.css(Color.accent)
        var stub = Style.space(12)
        function draw(card, onLeft) {
          if (canvas.entryBySlot[card.slot] === undefined) return
          var hot = canvas.hot(card.slot)
          var edgeX = onLeft ? canvas.leftCardsX + canvas.cardWidth : canvas.rightCardsX
          var edgeY = card.cardY + canvas.cardHeight / 2
          ctx.beginPath()
          ctx.strokeStyle = hot ? accent : dim
          ctx.lineWidth = hot ? 2 : 1
          ctx.moveTo(edgeX, edgeY)
          ctx.lineTo(edgeX + (onLeft ? stub : -stub), edgeY)
          ctx.lineTo(canvas.picturesX + card.x, card.y)
          ctx.stroke()
        }
        canvas.layout.left.forEach(function(card) { draw(card, true) })
        canvas.layout.right.forEach(function(card) { draw(card, false) })
      }
    }

    Row {
      x: canvas.picturesX
      spacing: canvas.viewGap

      Repeater {
        model: canvas.views

        delegate: Image {
          required property var modelData
          width: Model.viewWidth(modelData, canvas.pictureHeight)
          height: canvas.pictureHeight
          source: "file://" + modelData.image
          // Decoded once at a fixed size; resizing the overlay only rescales.
          sourceSize.height: 1024
          fillMode: Image.PreserveAspectFit
          asynchronous: true
          smooth: true
          mipmap: true
        }
      }
    }

    Repeater {
      model: canvas.layout.left.concat(canvas.layout.right)

      delegate: Item {
        id: dot
        required property var modelData
        readonly property bool hot: canvas.hot(modelData.slot)
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        width: Style.space(28)
        height: width
        x: canvas.picturesX + modelData.x - width / 2
        y: modelData.y - height / 2

        Rectangle {
          anchors.centerIn: parent
          width: dot.hot ? Style.space(14) : Style.space(10)
          height: width
          radius: width / 2
          color: dot.hot ? Color.accent : Color.menu.background
          border.width: Math.max(1, Style.normalBorderWidth)
          border.color: dot.hot ? Color.accent : Color.menu.text
        }

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: canvas.slotSelected(dot.modelData.slot)
          onContainsMouseChanged: canvas.slotHovered(dot.modelData.slot, containsMouse)
        }
      }
    }

    Repeater {
      model: canvas.layout.left

      delegate: SlotCard {
        required property var modelData
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        entry: canvas.entryFor(modelData.slot)
        x: canvas.leftCardsX
        y: modelData.cardY
      }
    }

    Repeater {
      model: canvas.layout.right

      delegate: SlotCard {
        required property var modelData
        visible: canvas.entryBySlot[modelData.slot] !== undefined
        entry: canvas.entryFor(modelData.slot)
        x: canvas.rightCardsX
        y: modelData.cardY
      }
    }
  }

  Flow {
    x: canvas.hasPicture ? canvas.leftCardsX : 0
    y: canvas.hasPicture ? content.y + canvas.pictureBlockHeight + Style.spacing.lg : 0
    width: canvas.hasPicture ? canvas.rightCardsX + canvas.cardWidth - canvas.leftCardsX : canvas.width
    spacing: canvas.cardGap

    Repeater {
      model: canvas.looseEntries

      delegate: SlotCard {
        required property var modelData
        entry: modelData
      }
    }
  }
}
