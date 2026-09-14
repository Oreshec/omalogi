import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// DPI stages and report rate. Stages are chips, like G HUB's sensitivity levels, marked
// with the default and DPI shift stage; the selected stage has a slider and an exact
// value.
Item {
  id: panel

  property var draft: null
  property var bounds: ({ min: 100, max: 25600, step: 50 })
  property var rates: []
  property int stage: 0
  // The value under the slider while it is dragged, or 0.
  property int dragDpi: 0

  // `immediate` is false for typed values, which save after a pause.
  signal edited(var draft, bool immediate)

  readonly property var stages: draft ? draft.dpiStages : []
  readonly property int current: Math.max(0, Math.min(stage, stages.length - 1))
  readonly property int currentDpi: stages.length > 0 ? stages[current] : 0
  readonly property string problem: draft ? Model.draftProblem(draft) : ""

  function setCurrent(dpi, immediate) {
    if (!panel.draft) return
    var snapped = Math.round(dpi / panel.bounds.step) * panel.bounds.step
    snapped = Math.max(panel.bounds.min, Math.min(panel.bounds.max, snapped))
    if (snapped === panel.currentDpi) return
    panel.edited(Model.setStage(panel.draft, panel.current, snapped), immediate)
  }

  component Label: Text {
    textFormat: Text.PlainText
    color: Color.menu.text
    elide: Text.ElideRight
    font.family: Style.font.menuFamily
    font.pixelSize: Style.font.body
  }

  Column {
    width: parent.width
    spacing: Style.spacing.xxl

    Column {
      width: parent.width
      spacing: Style.spacing.md

      PanelSectionHeader {
        text: "DPI stages"
        foreground: Color.menu.text
      }

      Label {
        width: parent.width
        opacity: 0.6
        wrapMode: Text.Wrap
        text: "The DPI buttons step through these stages, up to five. Select one to change it."
      }

      Row {
        spacing: Style.spacing.md

        Repeater {
          model: panel.stages

          delegate: Rectangle {
            id: chip
            required property var modelData
            required property int index
            readonly property bool selected: index === panel.current
            readonly property bool isDefault: panel.draft !== null && modelData === panel.draft.defaultDpi
            readonly property bool isShift: panel.draft !== null && modelData === panel.draft.shiftDpi

            width: Style.space(108)
            height: Style.space(68)
            radius: Style.cornerRadius
            color: selected ? Util.alpha(Color.accent, 0.14) : Util.alpha(Color.menu.text, 0.04)
            border.width: selected ? Math.max(2, Style.normalBorderWidth) : Math.max(1, Style.normalBorderWidth)
            border.color: selected ? Color.accent : Util.alpha(Color.menu.text, 0.18)

            Column {
              anchors.centerIn: parent
              spacing: Style.spacing.xxs

              Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: String(chip.modelData)
                font.pixelSize: Style.font.title
                font.bold: true
              }

              Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: [chip.isDefault ? "Default" : "", chip.isShift ? "Shift" : ""]
                  .filter(function(tag) { return tag !== "" })
                  .join(" · ") || "Stage " + (chip.index + 1)
                color: chip.isDefault || chip.isShift ? Color.accent : Color.menu.text
                opacity: chip.isDefault || chip.isShift ? 1 : 0.5
                font.pixelSize: Style.font.caption
              }
            }

            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: panel.stage = chip.index
            }
          }
        }

        Rectangle {
          visible: panel.draft !== null && panel.stages.length < 5
          width: Style.space(68)
          height: Style.space(68)
          radius: Style.cornerRadius
          color: addArea.containsMouse ? Util.alpha(Color.menu.text, 0.08) : "transparent"
          border.width: Math.max(1, Style.normalBorderWidth)
          border.color: Util.alpha(Color.menu.text, 0.25)

          Label {
            anchors.centerIn: parent
            text: "󰐕"
            font.family: Style.font.family
            font.pixelSize: Style.font.icon
          }

          MouseArea {
            id: addArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: {
              var next = Model.addStage(panel.draft, Model.nextStageDpi(panel.draft, panel.bounds))
              panel.stage = next.dpiStages.length - 1
              panel.edited(next, true)
            }
          }
        }
      }
    }

    Column {
      width: parent.width
      spacing: Style.spacing.md
      visible: panel.stages.length > 0

      PanelSectionHeader {
        text: "Stage " + (panel.current + 1) + "  ·  " + (panel.dragDpi > 0 ? panel.dragDpi : panel.currentDpi) + " DPI"
        foreground: Color.menu.text
      }

      Row {
        width: parent.width
        spacing: Style.spacing.lg

        PanelSlider {
          anchors.verticalCenter: parent.verticalCenter
          width: parent.width - field.width - parent.spacing
          height: Style.spacing.controlHeight
          // Logarithmic: the slider moves in positions, Model.js converts them to DPI.
          minimum: 0
          maximum: Model.SLIDER_STEPS
          step: 1
          integer: true
          value: Model.dpiToPosition(panel.currentDpi, panel.bounds)
          trackColor: Util.alpha(Color.menu.text, 0.18)
          fillColor: Color.accent
          knobColor: Color.menu.text
          tickColor: Color.menu.background
          onMoved: function(position) { panel.dragDpi = Model.positionToDpi(position, panel.bounds) }
          onReleased: function(position) {
            panel.dragDpi = 0
            panel.setCurrent(Model.positionToDpi(position, panel.bounds), true)
          }
        }

        NumberField {
          id: field
          anchors.verticalCenter: parent.verticalCenter
          from: panel.bounds.min
          to: panel.bounds.max
          stepSize: panel.bounds.step
          value: panel.currentDpi
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onModified: function(value) { panel.setCurrent(value, false) }
        }
      }

      Label {
        opacity: 0.6
        text: panel.bounds.min + "–" + panel.bounds.max + " DPI in steps of " + panel.bounds.step
        font.pixelSize: Style.font.caption
      }

      Row {
        spacing: Style.spacing.md

        Button {
          text: "Set as default"
          bordered: true
          enabled: panel.draft !== null && panel.currentDpi !== panel.draft.defaultDpi
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: panel.edited(Model.setField(panel.draft, "defaultDpi", panel.currentDpi), true)
        }

        Button {
          text: "Set as DPI shift"
          bordered: true
          enabled: panel.draft !== null && panel.currentDpi !== panel.draft.shiftDpi
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: panel.edited(Model.setField(panel.draft, "shiftDpi", panel.currentDpi), true)
        }

        Button {
          text: "Remove stage"
          bordered: true
          enabled: panel.stages.length > 1
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: {
            var next = Model.removeStage(panel.draft, panel.current)
            panel.stage = Math.max(0, panel.current - 1)
            panel.edited(next, true)
          }
        }
      }

      Label {
        visible: panel.problem !== ""
        width: parent.width
        wrapMode: Text.Wrap
        text: panel.problem
        color: Color.urgent
      }
    }

    Column {
      width: parent.width
      spacing: Style.spacing.md

      PanelSectionHeader {
        text: "Report rate"
        foreground: Color.menu.text
      }

      ButtonGroup {
        options: panel.rates.map(function(hz) { return { value: String(hz), label: hz + " Hz" } })
        value: panel.draft && panel.draft.rateHz !== null ? String(panel.draft.rateHz) : ""
        foreground: Color.menu.text
        fontFamily: Style.font.menuFamily
        onChanged: function(value) { panel.edited(Model.setField(panel.draft, "rateHz", Number(value)), true) }
      }

      Label {
        width: parent.width
        opacity: 0.6
        wrapMode: Text.Wrap
        text: "How often the mouse reports its movement. Higher is smoother and costs a little more CPU."
      }
    }
  }
}
