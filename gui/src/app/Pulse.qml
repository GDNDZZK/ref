import QtQuick
import Qutie

// Loops `target`'s opacity between `dimmed` and 1 while `running`. On stop it eases
// straight back to 1 from wherever the pulse was. alwaysRunToEnd would instead finish
// the current cycle: up to one more full blink after the work is done.
Item {
	id: pulse

	required property Item target
	property bool running: false
	property real dimmed: 0.5
	property int period: Motion.pulse // ms for one dim-and-back cycle

	SequentialAnimation {
		running: pulse.running
		loops: Animation.Infinite
		NumberAnimation { target: pulse.target; property: "opacity"; to: pulse.dimmed; duration: pulse.period / 2; easing.type: Easing.InOutSine }
		NumberAnimation { target: pulse.target; property: "opacity"; to: 1; duration: pulse.period / 2; easing.type: Easing.InOutSine }
	}

	NumberAnimation {
		id: settle
		target: pulse.target
		property: "opacity"
		to: 1
		duration: Motion.standard
		easing.type: Motion.ease
	}

	onRunningChanged: running ? settle.stop() : settle.restart()
}
