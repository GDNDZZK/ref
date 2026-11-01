import QtQuick
import RefGui

// The state of a session as a dot: idle is dim, work pulses in the accent color, waiting for
// permission or an answer and retrying pulse in the warning color, a failed last turn is a solid danger
// dot. A turn that ends plays the Flourish.
Item {
	id: root

	property string activity: Activity.idle
	property bool hasError: false

	readonly property bool working: Activity.isWorking(activity)
	readonly property color dotColor: colorFor(activity, hasError)

	function colorFor(state, errored) {
		if (Activity.isFailed(state, errored))
			return Theme.danger
		if (Activity.needsAttention(state))
			return Theme.warning
		if (Activity.isWorking(state))
			return Theme.accent
		return Theme.idleDot
	}

	implicitWidth: Theme.statusDotSize
	implicitHeight: Theme.statusDotSize

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
