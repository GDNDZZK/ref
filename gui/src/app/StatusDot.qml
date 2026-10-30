import QtQuick
import Qutie

// Tool status: pulses while running, plays a Flourish in its final color on finishing.
Item {
	id: status

	property bool running: false
	property bool failed: false

	readonly property color stateColor: running ? Theme.accent : failed ? Theme.danger : Theme.success

	implicitWidth: 8
	implicitHeight: 8

	Flourish {
		id: flourish
		target: dot
		color: status.stateColor
	}

	Rectangle {
		id: dot
		anchors.fill: parent
		radius: width / 2
		color: status.stateColor

		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	Pulse {
		target: dot
		running: status.running
		dimmed: 0.5
	}

	onRunningChanged: if (!running) flourish.play()
}
