// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Small borderless button with a text label.
// Only the face shrinks on press; the handlers stay on the unscaled root (see CircleButton).
Item {
	id: button

	property alias text: label.text
	signal clicked()

	implicitWidth: label.implicitWidth + 16
	implicitHeight: 24

	Behavior on implicitWidth { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

	Rectangle {
		id: face
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: hover.hovered ? Theme.surfaceHover : "transparent"
		scale: tap.pressed ? 0.94 : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			textFormat: Text.PlainText
			id: label
			anchors.centerIn: parent
			color: hover.hovered ? Theme.text : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}

	HoverHandler {
		id: hover
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.name: text
	Accessible.onPressAction: clicked()
}
