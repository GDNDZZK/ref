pragma Singleton
import QtQuick
import Qutie

// Timing for every animation.
// Frequent interactions use a short exponential ease-out and never overshoot.
// Overshoot (emphasisEase) is reserved for rare moments such as switching themes.
QtObject {
	readonly property int quick: 120    // hover, press, focus
	readonly property int standard: 260 // enter, expand, collapse, reflow
	readonly property int slow: 480     // theme crossfade, turn divider, releasing held space
	readonly property int pulse: 1100   // period of the looping "working" pulse
	readonly property int calmPulse: 1800 // slower pulse for text, where a fast blink is distracting to read

	readonly property int ease: Easing.OutExpo
	readonly property int emphasisEase: Easing.OutBack
	// Slow start, then away: an ease-out spends nearly all of its motion in the first
	// frames, so a short exit reads as a cut. Used where a glyph's exit should be seen.
	readonly property int exitEase: Easing.InCubic

	readonly property real enterDrop: 48 // px a new block drops into place while it fades in

	// Exponential chases track targets that keep moving: the scroll following the end
	// or a wheel target, a block's growing height. Each frame covers
	// 1 - e^(-frameTime / followLag) of the remaining distance, so a moving target
	// never restarts the motion and the speed does not depend on the frame rate.
	// Scroll chases round each step to whole pixels on top of this (ChatView.scrollStep).
	readonly property real followLag: 70
	readonly property real chaseSnap: 0.5 // px; closer than this counts as arrived

	function arrived(current, target) {
		return Math.abs(target - current) < chaseSnap
	}

	// One frame of a chase; frameTime in seconds, as FrameAnimation reports it.
	function chase(current, target, frameTime) {
		if (arrived(current, target))
			return target
		return current + (target - current) * (1 - Math.exp(-frameTime * 1000 / followLag))
	}
}
