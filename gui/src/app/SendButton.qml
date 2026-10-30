// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Round send button that morphs into stop while a reply is running.
// Into stop the glyphs swap with the usual ease-out; back to send, the square turns
// away with exitEase, so the end of a reply is seen rather than cut.
// Only the face shrinks on press; the handlers stay on the unscaled root (see CircleButton).
Item {
	id: button

	property bool busy: false
	property bool canSend: false
	property color activeColor: Theme.accent
	signal clicked()

	readonly property bool active: busy || canSend

	width: 32
	height: 32

	Rectangle {
		id: face
		anchors.fill: parent
		radius: width / 2
		color: button.active ? button.activeColor : Theme.surfaceHover
		scale: tap.pressed ? 0.9 : 1

		Behavior on color { ColorAnimation { duration: Motion.slow; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Chevron {
			id: arrow
			anchors.centerIn: parent
			width: 16
			height: 16
			rotation: -90 // points up
			color: button.active ? Theme.onAccent : Theme.textMuted
			strokeWidth: 2

			Behavior on color { ColorAnimation { duration: Motion.slow; easing.type: Motion.ease } }
		}

		Rectangle {
			id: stopGlyph
			anchors.centerIn: parent
			width: 10
			height: 10
			radius: 2
			color: Theme.onAccent
			opacity: 0
			scale: 0.5
			rotation: -90
		}
	}

	states: State {
		name: "busy"
		when: button.busy
		PropertyChanges {
			arrow.opacity: 0
			arrow.scale: 0.5
			stopGlyph.opacity: 1
			stopGlyph.scale: 1
			stopGlyph.rotation: 0
		}
	}
	transitions: [
		Transition {
			to: "busy"
			NumberAnimation {
				targets: [arrow, stopGlyph]
				properties: "opacity,scale,rotation"
				duration: Motion.standard
				easing.type: Motion.ease
			}
		},
		Transition {
			from: "busy"
			NumberAnimation {
				targets: [arrow, stopGlyph]
				properties: "opacity,scale,rotation"
				duration: Motion.quick
				easing.type: Motion.exitEase
			}
		}
	]

	HoverHandler { cursorShape: button.active ? Qt.PointingHandCursor : Qt.ArrowCursor }
	TapHandler {
		id: tap
		enabled: button.active
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.name: busy ? qsTr("Stop") : qsTr("Send")
	Accessible.onPressAction: if (active) clicked()
}
