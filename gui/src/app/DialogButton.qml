import QtQuick
import RefGui

// Button of dialogs and banners. `kind` is "primary", "secondary" or "danger".
// Only the face shrinks on press; the handlers stay on the unscaled root (see CircleButton).
Item {
	id: button

	property string kind: "secondary"
	property alias text: label.text
	property bool small: false

	signal clicked()

	readonly property bool filled: kind !== "secondary"
	readonly property color fill: kind === "danger" ? Theme.danger : kind === "primary" ? Theme.accent : Theme.surface

	implicitWidth: label.implicitWidth + 2 * 18
	implicitHeight: small ? 28 : Theme.buttonHeight
	opacity: enabled ? 1 : 0.4

	Rectangle {
		id: face
		anchors.fill: parent
		radius: height / 2
		color: hover.hovered && button.enabled ? (button.filled ? Qt.lighter(button.fill, 1.08) : Theme.surfaceHover) : button.fill
		border.color: button.filled ? "transparent" : Theme.border
		scale: tap.pressed && button.enabled ? 0.94 : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			id: label
			anchors.centerIn: parent
			color: button.filled ? Theme.onAccent : Theme.text
			font.pixelSize: Theme.fontSize - 1
		}
	}

	HoverHandler {
		id: hover
		enabled: button.enabled
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		enabled: button.enabled
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.name: text
	Accessible.onPressAction: if (enabled) clicked()
}
