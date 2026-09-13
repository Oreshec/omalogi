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

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

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
    onPressed: function(mouseButton) {
      Util.execArgv(["omarchy-shell", "shell", "toggle", "io.github.elberacasa.omalogi", "{}"])
    }
  }
}
