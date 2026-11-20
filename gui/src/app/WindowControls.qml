pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Window controls for frameless windows (minimize, maximize/restore, close).
Row {
	id: controls

	required property bool maximized

	signal minimizeRequested()
	signal maximizeRequested()
	signal closeRequested()

	readonly property int buttonSize: Theme.buttonHeight
	readonly property real iconSpan: 10
	readonly property real iconStroke: 1.5

	spacing: Theme.windowButtonGap

	component ControlButton: Item {
		id: button

		property bool danger: false
		property color glyphColor: danger && hover.hovered ? Theme.dangerText : (hover.hovered ? Theme.text : Theme.textMuted)
		readonly property color faceColor: face.color
		default property alias glyph: face.data

		signal clicked()

		implicitWidth: controls.buttonSize
		implicitHeight: controls.buttonSize

		Rectangle {
			id: face
			anchors.fill: parent
			radius: width / 2
			color: button.danger && hover.hovered ? Theme.errorSurface : (hover.hovered ? Theme.surfaceHover : Theme.surface)
			border.color: button.danger && hover.hovered ? Theme.errorBorder : Theme.border
			scale: tap.pressed ? 0.92 : 1

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
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
		Accessible.onPressAction: clicked()
	}

	ControlButton {
		id: minimizeButton
		Accessible.name: qsTr("Minimize")
		onClicked: controls.minimizeRequested()

		Item {
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan

			Rectangle {
				anchors.horizontalCenter: parent.horizontalCenter
				y: parent.height / 2 + 2 - height / 2
				width: controls.iconSpan - 2 + controls.iconStroke
				height: controls.iconStroke
				radius: height / 2
				color: minimizeButton.glyphColor
			}
		}
	}

	ControlButton {
		id: maximizeButton
		Accessible.name: controls.maximized ? qsTr("Restore") : qsTr("Maximize")
		onClicked: controls.maximizeRequested()

		Item {
			id: maximizeIcon
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan

			readonly property real strokeInset: controls.iconStroke / 2
			readonly property real restoreOrigin: 1 - strokeInset
			readonly property real restoreOffset: 2.5
			readonly property real restoreSize: controls.iconSpan - 4 + controls.iconStroke
			readonly property real maximizeSize: controls.iconSpan - 3 + controls.iconStroke

			Rectangle {
				visible: controls.maximized
				x: maximizeIcon.restoreOrigin + maximizeIcon.restoreOffset
				y: maximizeIcon.restoreOrigin
				width: maximizeIcon.restoreSize
				height: maximizeIcon.restoreSize
				radius: 1
				color: "transparent"
				border.width: controls.iconStroke
				border.color: maximizeButton.glyphColor
			}

			Rectangle {
				x: controls.maximized ? maximizeIcon.restoreOrigin : (maximizeIcon.width - width) / 2
				y: controls.maximized ? maximizeIcon.restoreOrigin + maximizeIcon.restoreOffset : (maximizeIcon.height - height) / 2
				width: controls.maximized ? maximizeIcon.restoreSize : maximizeIcon.maximizeSize
				height: width
				radius: 1
				color: controls.maximized ? maximizeButton.faceColor : "transparent"
				border.width: controls.iconStroke
				border.color: maximizeButton.glyphColor
			}
		}
	}

	ControlButton {
		id: closeButton
		danger: true
		Accessible.name: qsTr("Close")
		onClicked: controls.closeRequested()

		Item {
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan

			Repeater {
				model: [45, -45]

				delegate: Rectangle {
					required property int modelData
					anchors.centerIn: parent
					width: (controls.iconSpan - 3) * Math.SQRT2 + controls.iconStroke
					height: controls.iconStroke
					radius: height / 2
					rotation: modelData
					antialiasing: true
					color: closeButton.glyphColor
				}
			}
		}
	}
}
