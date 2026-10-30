// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// A small notice pill at the top center: connection trouble, the last failure.
// Fades out when `text` is empty.
Rectangle {
	id: pill

	property string text: ""
	readonly property bool shown: text !== ""

	anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 14 }
	width: Math.min(label.implicitWidth + 2 * 14, parent.width - 2 * 80) // clear of the corner buttons
	height: label.implicitHeight + 2 * 6
	radius: height / 2
	color: Theme.surface
	border.color: Theme.border
	opacity: shown ? 1 : 0
	visible: opacity > 0

	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Text {
		id: label
		x: 14
		y: 6
		width: parent.width - 2 * 14
		text: pill.text
		elide: Text.ElideRight
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}
}
