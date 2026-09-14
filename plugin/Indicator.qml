import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Bar indicator: the active onboard profile, read from the omalogi daemon's state file.
// Clicking opens the Omalogi overlay.
BarWidget {
  id: root
  moduleName: "io.github.elberacasa.omalogi"

  // The daemon's published state, or null when it is not running.
  property var daemon: null
  readonly property string activeProfile: daemon && daemon.connected && daemon.active_profile
    ? String(daemon.active_profile) : ""

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // The mark's wheel rolls for a moment when the profile changes, so a switch is seen.
  onActiveProfileChanged: if (activeProfile !== "") rolling.restart()

  Timer {
    id: rolling
    interval: 1100
  }

  FileView {
    path: Quickshell.env("XDG_RUNTIME_DIR") + "/omalogi/state.json"
    watchChanges: true
    printErrors: false
    onLoaded: root.daemon = Model.parseJson(text())
    // text() is stale inside the change signal, so re-read and parse in onLoaded.
    onFileChanged: reload()
    onLoadFailed: root.daemon = null
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: Model.indicatorText(root.daemon)
    active: Model.indicatorActive(root.daemon)
    tooltipText: Model.indicatorTooltip(root.daemon)
    // The pixel mark in the bar's colours; its wheel is lit while a rule chose the profile.
    iconComponent: Component {
      PixelMark {
        ink: button.foreground
        accent: button.active ? Color.accent : button.foreground
        busy: rolling.running
        opacity: root.activeProfile !== "" ? 1 : 0.5
      }
    }
    onPressed: function(mouseButton) {
      Util.execArgv(["omarchy-shell", "shell", "toggle", "io.github.elberacasa.omalogi", "{}"])
    }
  }
}
