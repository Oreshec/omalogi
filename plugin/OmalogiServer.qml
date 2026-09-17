import QtQuick
import Quickshell.Io
import "Model.js" as Model

// A long-lived `omalogi serve`. Requests go to its stdin as JSON lines, and each reply
// line on stdout goes to the callback registered for its id. The process starts with
// the first request; if it exits, every request still waiting fails with the reason.
Item {
  id: server

  readonly property bool running: process.running
  property int inFlight: 0
  property int nextId: 1
  // Request id -> function(ok, resultOrError, kind); `kind` names errors the overlay acts on.
  property var callbacks: ({})
  // Lines written before the process has started.
  property var queue: []
  property bool started: false

  // The process ended while requests were waiting.
  signal failed(string message)

  function request(command, callback) {
    var id = server.nextId++
    server.callbacks[id] = callback
    server.inFlight++
    var line = JSON.stringify(Object.assign({ id: id }, command)) + "\n"
    if (server.started) {
      process.write(line)
    } else {
      server.queue.push(line)
      if (!process.running) process.running = true
    }
    return id
  }

  function stop() {
    if (process.running) process.running = false
  }

  function settle(id, ok, value, kind) {
    var callback = server.callbacks[id]
    if (callback === undefined) return
    delete server.callbacks[id]
    server.inFlight = Math.max(0, server.inFlight - 1)
    callback(ok, value, kind || "")
  }

  function failAll(message) {
    // Queued lines must never reach a later process after their requests have failed.
    server.queue = []
    Object.keys(server.callbacks).forEach(function(id) { server.settle(id, false, message) })
  }

  Process {
    id: process
    // A missing binary exits 127 through the shell instead of failing to start silently.
    command: ["sh", "-c", "command -v omalogi >/dev/null 2>&1 || exit 127; exec omalogi serve"]
    stdinEnabled: true

    onStarted: {
      server.started = true
      var lines = server.queue
      server.queue = []
      lines.forEach(function(line) { process.write(line) })
    }

    stdout: SplitParser {
      onRead: function(line) {
        var reply = Model.parseJson(line)
        if (reply === null || reply.id === null || reply.id === undefined) return
        var ok = reply.ok === true
        server.settle(reply.id, ok, ok ? reply.result : String(reply.error), reply.kind || "")
      }
    }

    stderr: StdioCollector {
      id: errors
    }

    onExited: function(exitCode) {
      server.started = false
      var waiting = server.inFlight > 0 || server.queue.length > 0
      var message = Model.errorMessage(errors.text, exitCode)
      server.failAll(message)
      if (waiting) server.failed(message)
    }
  }
}
