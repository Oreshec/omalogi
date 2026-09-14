import QtQuick
import qs.Commons
import "Icons.js" as Icons

// A Lucide icon (see Icons.js) drawn in a theme color.
Image {
  id: icon

  property string name: ""
  property color tint: Color.menu.text
  property int size: Style.font.icon

  width: size
  height: size
  sourceSize.width: size * 2
  sourceSize.height: size * 2
  fillMode: Image.PreserveAspectFit
  smooth: true
  source: Icons.source(name, tint)
}
