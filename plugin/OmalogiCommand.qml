import QtQuick
import Quickshell.Io

// Runs one omalogi command and emits `finished` once its exit status and both
// output streams are in, whichever arrives last.
Item {
  id: runner

  property var args: []
  property bool pending: false
  property int exitCode: -1
  property bool exited: false
  property bool outDone: false
  property bool errDone: false

  readonly property bool running: runner.pending

  signal finished(int exitCode, string stdout, string stderr)

  function start(args) {
    if (runner.pending) return false
    runner.args = args
    runner.exitCode = -1
    runner.exited = false
    runner.outDone = false
    runner.errDone = false
    runner.pending = true
    process.running = true
    return true
  }

  function settle() {
    if (!runner.pending || !runner.exited || !runner.outDone || !runner.errDone) return
    runner.pending = false
    runner.finished(runner.exitCode, output.text, errors.text)
  }

  Process {
    id: process
    // A missing binary exits 127 through the shell instead of failing to start silently.
    command: ["sh", "-c", "command -v omalogi >/dev/null 2>&1 || exit 127; exec omalogi \"$@\"", "omalogi"].concat(runner.args)

    stdout: StdioCollector {
      id: output
      onStreamFinished: {
        runner.outDone = true
        runner.settle()
      }
    }

    stderr: StdioCollector {
      id: errors
      onStreamFinished: {
        runner.errDone = true
        runner.settle()
      }
    }

    onExited: function(exitCode) {
      runner.exitCode = exitCode
      runner.exited = true
      runner.settle()
    }
  }
}
