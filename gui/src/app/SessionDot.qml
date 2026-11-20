import QtQuick
import RefGui

// The state of a session as a dot: idle is dim, work pulses in the accent color, waiting for
// permission or an answer and retrying pulse in the warning color, a failed last turn is a solid danger
// dot. A turn that ends plays the Flourish.
Item {
	id: root

	property string activity: Activity.idle
	property bool hasError: false
	property color tint: "transparent"

	readonly property bool working: Activity.isWorking(activity)
	readonly property color dotColor: colorFor(activity, hasError, tint)

	function colorFor(state, errored, tintColor) {
		if (Activity.isFailed(state, errored))
			return Theme.danger
		if (Activity.needsAttention(state))
			return Theme.warning
		const hasTint = tintColor !== undefined && tintColor.a > 0
		if (Activity.isWorking(state))
			return hasTint ? tintColor : Theme.accent
		return hasTint ? Qt.alpha(tintColor, 0.5) : Theme.idleDot
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
		running: root.working && root.visible
	}

	onWorkingChanged: if (!working && !hasError) flourish.play()
}
