import QtQuick
import RefGui

// The state of a session as a dot: idle is dim, work pulses in the accent color, waiting for
// permission or an answer and retrying pulse in the warning color, a failed last turn is a solid danger
// dot. A turn that ends plays the Flourish.
Item {
	id: root

	property string activity: "idle" // idle, thinking, running, waiting_permission, waiting_question, retrying or compacting
	property bool hasError: false

	readonly property bool working: activity !== "idle"
	readonly property bool needsAttention: activity === "waiting_permission" || activity === "waiting_question" || activity === "retrying"
	readonly property color dotColor: hasError && !working ? Theme.danger : needsAttention ? Theme.warning : working ? Theme.accent : Qt.alpha(Theme.textMuted, 0.5)

	implicitWidth: 8
	implicitHeight: 8

	Flourish {
		id: flourish
		target: dot
		color: Theme.success
	}

	Rectangle {
		id: dot
		anchors.fill: parent
		radius: width / 2
		color: root.dotColor

		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	Pulse {
		target: dot
		running: root.working
		dimmed: 0.5
	}

	onWorkingChanged: if (!working && !hasError) flourish.play()
}
