import QtQuick
import RefGui

// Button of dialogs and banners. `kind` is one of the Kind values.
// Only the face shrinks on press; the handlers stay on the unscaled root (see CircleButton).
Item {
	id: button

	enum Kind { Secondary, Primary, Danger }

	property int kind: DialogButton.Secondary
	property alias text: label.text
	property bool small: false

	signal clicked()

	readonly property bool filled: kind !== DialogButton.Secondary
	readonly property color fill: fillFor(kind)
	readonly property color hoverFill: filled ? Qt.lighter(fill, 1.08) : Theme.surfaceHover

	// The resting fill of a kind.
	function fillFor(kind) {
		if (kind === DialogButton.Danger)
			return Theme.danger
		return kind === DialogButton.Primary ? Theme.accent : Theme.surface
	}

	implicitWidth: label.implicitWidth + 2 * 18
	implicitHeight: small ? 28 : Theme.buttonHeight
	opacity: enabled ? 1 : Theme.disabledOpacity

	Rectangle {
		id: face
		anchors.fill: parent
		radius: height / 2
		color: hover.hovered && button.enabled ? button.hoverFill : button.fill
		border.color: button.filled ? "transparent" : Theme.border
		scale: tap.pressed && button.enabled ? Motion.pressedScaleButton : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			textFormat: Text.PlainText
			id: label
			anchors.centerIn: parent
			color: button.filled ? Theme.onAccent : Theme.text
			font.pixelSize: Theme.secondaryFontSize
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
