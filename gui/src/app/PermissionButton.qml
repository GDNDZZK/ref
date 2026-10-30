import QtQuick
import RefGui

// An answer to a permission request: icon, label and the key that gives it.
Item {
	id: button

	required property string iconName
	required property string label
	required property string keyName
	required property color tint

	signal clicked()

	readonly property int buttonHeight: 30
	readonly property int innerPadding: 10
	readonly property int innerSpacing: 6
	readonly property real borderOpacity: 0.5
	readonly property real hoverOpacity: 0.12
	readonly property real pressedOpacity: 0.22
	readonly property int keyCapSize: 18

	implicitWidth: content.width + 2 * innerPadding
	implicitHeight: buttonHeight

	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: Qt.alpha(button.tint, tap.pressed ? button.pressedOpacity : hover.hovered ? button.hoverOpacity : 0)
		border.color: Qt.alpha(button.tint, button.borderOpacity)

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	Row {
		id: content
		anchors.centerIn: parent
		spacing: button.innerSpacing

		Icon {
			anchors.verticalCenter: parent.verticalCenter
			name: button.iconName
			color: button.tint
		}
		Text {
			anchors.verticalCenter: parent.verticalCenter
			text: button.label
			color: Theme.text
			font.pixelSize: Theme.fontSize - 2
		}
		Rectangle {
			anchors.verticalCenter: parent.verticalCenter
			width: button.keyCapSize
			height: button.keyCapSize
			radius: 4
			color: "transparent"
			border.color: button.tint

			Text {
				anchors.centerIn: parent
				text: button.keyName
				color: button.tint
				font.family: Theme.monoFamily
				font.pixelSize: 11
			}
		}
	}

	HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
	TapHandler { id: tap; onTapped: button.clicked() }
	Accessible.role: Accessible.Button
	Accessible.name: label
	Accessible.onPressAction: clicked()
}
