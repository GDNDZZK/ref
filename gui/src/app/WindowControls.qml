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

		Canvas {
			id: minimizeCanvas
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan
			renderTarget: Canvas.Image

			onPaint: {
				const ctx = getContext("2d")
				ctx.reset()
				ctx.strokeStyle = minimizeButton.glyphColor
				ctx.lineWidth = controls.iconStroke
				ctx.lineCap = "round"
				const y = height / 2 + 2
				ctx.beginPath()
				ctx.moveTo(1, y)
				ctx.lineTo(width - 1, y)
				ctx.stroke()
			}
			Connections {
				target: minimizeButton
				function onGlyphColorChanged() { minimizeCanvas.requestPaint() }
			}
		}
	}

	ControlButton {
		id: maximizeButton
		Accessible.name: controls.maximized ? qsTr("Restore") : qsTr("Maximize")
		onClicked: controls.maximizeRequested()

		Canvas {
			id: maximizeCanvas
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan
			renderTarget: Canvas.Image

			onPaint: {
				const ctx = getContext("2d")
				ctx.reset()
				ctx.strokeStyle = maximizeButton.glyphColor
				ctx.lineWidth = controls.iconStroke
				ctx.lineJoin = "round"
				if (controls.maximized) {
					const size = width - 4
					const offset = 2.5
					ctx.strokeRect(1, 1 + offset, size, size)
					ctx.beginPath()
					ctx.moveTo(1 + offset, 1 + offset)
					ctx.lineTo(1 + offset, 1)
					ctx.lineTo(1 + offset + size, 1)
					ctx.lineTo(1 + offset + size, 1 + size)
					ctx.lineTo(1 + size, 1 + size)
					ctx.stroke()
				} else {
					ctx.strokeRect(1.5, 1.5, width - 3, height - 3)
				}
			}
			Connections {
				target: maximizeButton
				function onGlyphColorChanged() { maximizeCanvas.requestPaint() }
			}
			Connections {
				target: controls
				function onMaximizedChanged() { maximizeCanvas.requestPaint() }
			}
		}
	}

	ControlButton {
		id: closeButton
		danger: true
		Accessible.name: qsTr("Close")
		onClicked: controls.closeRequested()

		Canvas {
			id: closeCanvas
			anchors.centerIn: parent
			width: controls.iconSpan
			height: controls.iconSpan
			renderTarget: Canvas.Image

			onPaint: {
				const ctx = getContext("2d")
				ctx.reset()
				ctx.strokeStyle = closeButton.glyphColor
				ctx.lineWidth = controls.iconStroke
				ctx.lineCap = "round"
				const pad = 1.5
				ctx.beginPath()
				ctx.moveTo(pad, pad)
				ctx.lineTo(width - pad, height - pad)
				ctx.moveTo(width - pad, pad)
				ctx.lineTo(pad, height - pad)
				ctx.stroke()
			}
			Connections {
				target: closeButton
				function onGlyphColorChanged() { closeCanvas.requestPaint() }
			}
		}
	}
}
