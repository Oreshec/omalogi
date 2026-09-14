import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// DPI levels on one bar, as G HUB draws them: a node per level on a logarithmic scale,
// the default level filled and the DPI shift level ringed. Drag a node to change its
// DPI, drag it away from the bar to remove it, and click the bar to add a level.
Item {
  id: track

  property var draft: null
  property var bounds: ({ min: 100, max: 25600, step: 50 })
  property int selected: 0

  signal stageSelected(int index)
  signal edited(var draft, bool immediate)

  readonly property var nodes: draft ? Model.dpiNodes(draft, bounds) : []
  readonly property var ticks: Model.dpiTicks(bounds)
  readonly property int nodeSize: Style.space(18)
  readonly property real lineY: Style.space(52)
  readonly property real removeDistance: Style.space(44)

  // The level being dragged, where it is, and whether letting go removes it.
  property int dragIndex: -1
  property real dragFraction: 0
  property bool dragRemoving: false

  // Rows from top to bottom: level values, the bar, Default and Shift tags, the scale.
  readonly property real tagY: lineY + nodeSize - Style.space(2)
  readonly property real scaleY: lineY + Style.space(46)

  implicitHeight: scaleY + Style.space(22)

  function fractionAt(x) {
    return Math.max(0, Math.min(1, x / Math.max(1, track.width)))
  }

  component Caption: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.caption
  }

  // The bar itself; clicking it adds a level.
  Rectangle {
    id: line
    x: 0
    width: track.width
    y: track.lineY - height / 2
    height: Style.space(4)
    radius: height / 2
    color: Util.alpha(Color.menu.text, 0.14)
  }

  // The range the levels cover.
  Rectangle {
    visible: track.nodes.length > 1
    readonly property real low: track.nodes.length > 0 ? track.nodes[0].fraction : 0
    readonly property real high: track.nodes.length > 0 ? track.nodes[track.nodes.length - 1].fraction : 0
    x: low * track.width
    width: Math.max(0, (high - low) * track.width)
    y: line.y
    height: line.height
    radius: line.radius
    color: Util.alpha(Color.accent, 0.45)
  }

  MouseArea {
    x: 0
    width: track.width
    y: track.lineY - Style.space(16)
    height: Style.space(32)
    enabled: track.draft !== null && track.nodes.length < 5
    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
    onClicked: function(mouse) {
      var dpi = Model.dpiAtFraction(track.fractionAt(mouse.x), track.bounds)
      if (track.draft.dpiStages.indexOf(dpi) !== -1) return
      var next = Model.insertStage(track.draft, dpi)
      track.stageSelected(next.dpiStages.indexOf(dpi))
      track.edited(next, true)
    }
  }

  Repeater {
    model: track.ticks

    delegate: Item {
      required property var modelData
      x: modelData.fraction * track.width
      y: track.scaleY

      Rectangle {
        x: -width / 2
        y: -Style.space(8)
        width: Math.max(1, Style.normalBorderWidth)
        height: Style.space(5)
        color: Util.alpha(Color.menu.text, 0.3)
      }

      Caption {
        x: Math.max(-parent.x, Math.min(track.width - parent.x - width, -width / 2))
        opacity: 0.45
        text: String(parent.modelData.dpi)
      }
    }
  }

  Repeater {
    model: track.nodes

    delegate: Item {
      id: node
      required property var modelData
      readonly property bool dragging: track.dragIndex === modelData.index
      readonly property real fraction: dragging ? track.dragFraction : modelData.fraction
      readonly property int shownDpi: dragging ? Model.dpiAtFraction(track.dragFraction, track.bounds) : modelData.dpi
      readonly property bool isSelected: track.selected === modelData.index

      x: fraction * track.width
      y: 0
      z: isSelected || dragging ? 2 : 1
      opacity: dragging && track.dragRemoving ? 0.35 : 1

      Text {
        x: Math.max(-node.x, Math.min(track.width - node.x - width, -width / 2))
        y: track.lineY - track.nodeSize - Style.space(22)
        textFormat: Text.PlainText
        text: node.shownDpi
        color: node.modelData.isDefault ? Color.accent : Color.menu.text
        font.family: Style.font.menuFamily
        font.pixelSize: Style.font.body
        font.bold: node.isSelected
      }

      Rectangle {
        readonly property int size: node.isSelected || node.dragging ? Math.round(track.nodeSize * 1.3) : track.nodeSize
        x: -size / 2
        y: track.lineY - size / 2
        width: size
        height: size
        radius: size / 2
        color: node.modelData.isDefault ? Color.accent : Color.menu.background
        border.width: node.modelData.isShift ? Math.max(3, Style.space(3)) : Math.max(2, Style.normalBorderWidth)
        border.color: node.modelData.isShift || node.isSelected ? Color.accent : Color.menu.text

        Behavior on width { NumberAnimation { duration: 90 } }
      }

      Caption {
        x: Math.max(-node.x, Math.min(track.width - node.x - width, -width / 2))
        y: track.tagY
        visible: node.modelData.isDefault || node.modelData.isShift || node.dragging && track.dragRemoving
        color: node.dragging && track.dragRemoving ? Color.urgent : Color.accent
        text: node.dragging && track.dragRemoving
          ? "Remove"
          : [node.modelData.isDefault ? "Default" : "", node.modelData.isShift ? "Shift" : ""]
            .filter(function(tag) { return tag !== "" })
            .join(" · ")
      }

      MouseArea {
        x: -Style.space(20)
        y: track.lineY - Style.space(22)
        width: Style.space(40)
        height: Style.space(44)
        hoverEnabled: true
        preventStealing: true
        cursorShape: node.dragging ? Qt.ClosedHandCursor : Qt.OpenHandCursor

        onPressed: function(mouse) {
          track.stageSelected(node.modelData.index)
          track.dragIndex = node.modelData.index
          track.dragFraction = node.modelData.fraction
          track.dragRemoving = false
        }

        onPositionChanged: function(mouse) {
          if (!node.dragging) return
          var point = mapToItem(track, mouse.x, mouse.y)
          track.dragFraction = track.fractionAt(point.x)
          track.dragRemoving = track.nodes.length > 1 && Math.abs(point.y - track.lineY) > track.removeDistance
        }

        onReleased: {
          if (!node.dragging) return
          var index = node.modelData.index
          var before = node.modelData.dpi
          var removing = track.dragRemoving
          var dpi = Model.dpiAtFraction(track.dragFraction, track.bounds)
          track.dragIndex = -1
          track.dragRemoving = false
          if (removing) {
            track.stageSelected(Math.max(0, index - 1))
            track.edited(Model.removeStageKeepingRoles(track.draft, index), true)
          } else if (dpi !== before) {
            var moved = Model.sortStages(Model.setStage(track.draft, index, dpi))
            track.stageSelected(moved.dpiStages.indexOf(dpi))
            track.edited(moved, true)
          }
        }

        onCanceled: {
          track.dragIndex = -1
          track.dragRemoving = false
        }
      }
    }
  }
}
