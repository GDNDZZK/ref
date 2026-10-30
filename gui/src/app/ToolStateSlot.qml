import QtQuick
import RefGui

// The state of a tool call in a 16 x 16 slot: a pulsing dot while it works, a lock while it
// waits for permission, a draining ring while it waits to start, a settled dot when done or
// failed, an empty ring when it was denied or cancelled. A dot that settles plays a Flourish.
Item {
	id: slot

	required property string toolState
	property real runAt: 0

	readonly property bool working: toolState === "streaming" || toolState === "running"
	readonly property bool showsDot: working || toolState === "done" || toolState === "error"
	readonly property bool showsRing: toolState === "denied" || toolState === "cancelled"
	readonly property int dotSize: 8
	readonly property real ringStroke: 1.5
	property string previousState: toolState

	readonly property color dotColor: working ? Theme.accent : toolState === "error" ? Theme.danger : Theme.success

	implicitWidth: 16
	implicitHeight: 16

	onToolStateChanged: {
		const settles = toolState === "done" || toolState === "error"
		const wasActive = previousState === "streaming" || previousState === "running" || previousState === "delayed"
		if (settles && wasActive)
			flourish.play()
		previousState = toolState
	}

	Flourish {
		id: flourish
		target: dot
		color: slot.dotColor
	}

	Rectangle {
		id: dot
		anchors.centerIn: parent
		width: slot.dotSize
		height: slot.dotSize
		radius: width / 2
		color: slot.dotColor
		visible: opacity > 0
		opacity: slot.showsDot ? 1 : 0

		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	Pulse {
		target: dot
		running: slot.working
		dimmed: 0.5
	}

	Rectangle {
		anchors.centerIn: parent
		width: slot.dotSize
		height: slot.dotSize
		radius: width / 2
		color: "transparent"
		border.color: Theme.textMuted
		border.width: slot.ringStroke
		opacity: slot.showsRing ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	Icon {
		name: "lock"
		color: Theme.warning
		opacity: slot.toolState === "awaiting_permission" ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	CountdownRing {
		anchors.fill: parent
		runAt: slot.runAt
		running: slot.toolState === "delayed"
		opacity: running ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
}
