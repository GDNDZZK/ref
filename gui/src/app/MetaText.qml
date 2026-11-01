import QtQuick
import RefGui

// One fact in a message's meta line, as tall as the line so that the facts line up in a Flow.
Text {
	textFormat: Text.PlainText
	color: Theme.textMuted
	font.pixelSize: Theme.smallFontSize
	height: 24
	verticalAlignment: Text.AlignVCenter
}
