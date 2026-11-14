import QtQuick
import RefGui

// A small fact about a tool call: an exit code, a size, "truncated".
Rectangle {
	id: chip

	property alias text: label.text
	property color tint: Theme.textMuted // fill and border
	property color textTint: tint // label

	implicitWidth: label.implicitWidth + 2 * 8
	implicitHeight: 20
	radius: 4
	color: Qt.alpha(tint, 0.1)
	border.color: Qt.alpha(tint, 0.35)

	Text {
		textFormat: Text.PlainText
		id: label
		anchors.centerIn: parent
		color: chip.textTint
		font.family: Theme.monoFamily
		font.pixelSize: Theme.microFontSize
	}
}
