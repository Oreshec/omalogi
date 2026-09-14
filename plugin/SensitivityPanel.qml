import QtQuick
import qs.Commons
import qs.Ui
import "Model.js" as Model

// DPI levels and report rate, laid out like G HUB's sensitivity page: every level on
// one bar, the selected level's exact value and role below it.
Item {
  id: panel

  property var draft: null
  property var bounds: ({ min: 100, max: 25600, step: 50 })
  property var rates: []
  property int stage: 0

  // `immediate` is false for typed values, which save after a pause.
  signal edited(var draft, bool immediate)

  readonly property var stages: draft ? draft.dpiStages : []
  readonly property int current: Math.max(0, Math.min(stage, stages.length - 1))
  readonly property int currentDpi: stages.length > 0 ? stages[current] : 0
  readonly property string problem: draft ? Model.draftProblem(draft) : ""

  // Sets the selected level's DPI; levels re-sort and the selection follows the value.
  function setCurrent(dpi, immediate) {
    if (!panel.draft) return
    var snapped = Math.round(dpi / panel.bounds.step) * panel.bounds.step
    snapped = Math.max(panel.bounds.min, Math.min(panel.bounds.max, snapped))
    if (snapped === panel.currentDpi) return
    var next = Model.sortStages(Model.setStage(panel.draft, panel.current, snapped))
    panel.stage = next.dpiStages.indexOf(snapped)
    panel.edited(next, immediate)
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

    // The same title block as the Assignments page.
    Column {
      width: parent.width
      spacing: Style.spacing.xxs

      Label {
        width: parent.width
        text: "Sensitivity"
        font.pixelSize: Style.font.heading
        font.bold: true
      }

      Label {
        width: parent.width
        opacity: 0.6
        text: panel.stages.length === 1 ? "1 DPI level" : panel.stages.length + " DPI levels"
        font.pixelSize: Style.font.caption
      }
    }

    Column {
      width: parent.width
      spacing: Style.spacing.md

      PanelSectionHeader {
        text: "DPI levels"
        foreground: Color.menu.text
      }

      Label {
        width: parent.width
        opacity: 0.6
        wrapMode: Text.Wrap
        text: "Drag a level to change it, or away from the bar to remove it. Click the bar to add a level. The DPI buttons step through the levels from low to high."
      }

      DpiTrack {
        width: parent.width
        draft: panel.draft
        bounds: panel.bounds
        selected: panel.current
        onStageSelected: function(index) { panel.stage = index }
        onEdited: function(next, immediate) { panel.edited(next, immediate) }
      }

      Row {
        spacing: Style.spacing.lg
        visible: panel.stages.length > 0

        Label {
          anchors.verticalCenter: parent.verticalCenter
          text: "Level " + (panel.current + 1)
          font.bold: true
        }

        NumberField {
          anchors.verticalCenter: parent.verticalCenter
          from: panel.bounds.min
          to: panel.bounds.max
          stepSize: panel.bounds.step
          value: panel.currentDpi
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onModified: function(value) { panel.setCurrent(value, false) }
        }

        Button {
          anchors.verticalCenter: parent.verticalCenter
          text: "Set as default"
          bordered: true
          enabled: panel.draft !== null && panel.currentDpi !== panel.draft.defaultDpi
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: panel.edited(Model.setField(panel.draft, "defaultDpi", panel.currentDpi), true)
        }

        Button {
          anchors.verticalCenter: parent.verticalCenter
          text: "Set as DPI shift"
          bordered: true
          enabled: panel.draft !== null && panel.currentDpi !== panel.draft.shiftDpi
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: panel.edited(Model.setField(panel.draft, "shiftDpi", panel.currentDpi), true)
        }

        Button {
          anchors.verticalCenter: parent.verticalCenter
          text: "Add level"
          iconText: "󰐕"
          bordered: true
          visible: panel.stages.length < 5
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: {
            var dpi = Model.nextStageDpi(panel.draft, panel.bounds)
            if (panel.stages.indexOf(dpi) !== -1) dpi = Math.max(panel.bounds.min, dpi - panel.bounds.step)
            var next = Model.insertStage(panel.draft, dpi)
            panel.stage = next.dpiStages.indexOf(dpi)
            panel.edited(next, true)
          }
        }

        Button {
          anchors.verticalCenter: parent.verticalCenter
          text: "Remove"
          bordered: true
          enabled: panel.stages.length > 1
          opacity: enabled ? 1 : 0.4
          foreground: Color.menu.text
          fontFamily: Style.font.menuFamily
          onClicked: {
            var next = Model.removeStageKeepingRoles(panel.draft, panel.current)
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
