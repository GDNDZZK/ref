import QtQuick
import RefGui

// A small fact about a tool call: an exit code, a size, "truncated".
Rectangle {
	id: chip

	property alias text: label.text
	property color tint: Theme.textMuted

	implicitWidth: label.implicitWidth + 2 * 8
	implicitHeight: 20
	radius: 4
	color: Qt.alpha(tint, 0.1)
	border.color: Qt.alpha(tint, 0.35)

	Text {
		id: label
		anchors.centerIn: parent
		color: chip.tint
		font.family: Theme.monoFamily
		font.pixelSize: 11
	}
}
