// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// The "it happened" flourish, in two forms. Both spring `target` to full size with a
// wobble and send a ring in `color` out from its center.
//   play():  on a shown target. It draws in to `gatheredScale` first.
//   enter(): on a target appearing. It fades in from `entryScale` as it springs open.
//            Growing from nothing reads as an odd swelling dot; a partial size reads as
//            the item arriving.
//
// Place it before `target` in the same parent, so the ring passes behind the target.
// Everything stays within a few target diameters and moves nothing else, so it may
// overshoot even where it plays often.
Item {
	id: flourish

	required property Item target
	property color color: Theme.accent
	property real ringReach: 3 // ring's final diameter, in target diameters

	readonly property real gatheredScale: 0.6
	readonly property real entryScale: 0.4
	// OutElastic shape. Both forms peak around 1.35x, then dip to about 0.9x before
	// they settle; entry starts smaller, so it needs less amplitude for the same peak.
	readonly property real springAmplitude: 2
	readonly property real entrySpringAmplitude: 1.4
	readonly property real springPeriod: 0.35 // fraction of springDuration per wobble
	readonly property int springDuration: 720 // ms; the wobble needs longer than Motion.slow to die out

	readonly property int gatherDuration: Motion.quick // until play()'s spring starts

	function play() {
		entrance.stop()
		animation.restart()
	}

	function enter() {
		animation.stop()
		entrance.restart()
	}

	anchors.centerIn: target
	width: target.width
	height: target.height

	Rectangle {
		id: ring
		anchors.centerIn: parent
		width: flourish.width
		height: width
		radius: width / 2
		color: "transparent"
		border.color: flourish.color
		border.width: 1.5
		opacity: 0
	}

	SequentialAnimation {
		id: animation

		NumberAnimation { target: flourish.target; property: "scale"; to: flourish.gatheredScale; duration: flourish.gatherDuration; easing.type: Easing.InQuad }
		ParallelAnimation {
			NumberAnimation {
				target: flourish.target
				property: "scale"
				to: 1
				duration: flourish.springDuration
				easing.type: Easing.OutElastic
				easing.amplitude: flourish.springAmplitude
				easing.period: flourish.springPeriod
			}
			NumberAnimation { target: ring; property: "width"; from: flourish.width; to: flourish.width * flourish.ringReach; duration: Motion.slow; easing.type: Motion.ease }
			NumberAnimation { target: ring; property: "opacity"; from: 0.7; to: 0; duration: Motion.slow; easing.type: Motion.ease }
		}
	}

	ParallelAnimation {
		id: entrance

		NumberAnimation { target: flourish.target; property: "opacity"; from: 0; to: 1; duration: Motion.quick; easing.type: Motion.ease }
		NumberAnimation {
			target: flourish.target
			property: "scale"
			from: flourish.entryScale
			to: 1
			duration: flourish.springDuration
			easing.type: Easing.OutElastic
			easing.amplitude: flourish.entrySpringAmplitude
			easing.period: flourish.springPeriod
		}
		NumberAnimation { target: ring; property: "width"; from: flourish.width; to: flourish.width * flourish.ringReach; duration: Motion.slow; easing.type: Motion.ease }
		NumberAnimation { target: ring; property: "opacity"; from: 0.7; to: 0; duration: Motion.slow; easing.type: Motion.ease }
	}
}
