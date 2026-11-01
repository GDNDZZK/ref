import QtQuick
import RefGui

// True for a moment after flash(), so a copy button can show that the copy happened.
Timer {
	id: flag

	property bool copied: false

	function flash() {
		copied = true
		restart()
	}

	interval: Motion.copiedFeedbackMs
	onTriggered: copied = false
}
