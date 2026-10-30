import QtQuick
import RefGui

// Changes something under a snapshot of how it looked and fades the snapshot out, so that
// everything on screen crossfades at once without animating each item.
Image {
	id: shot

	property var grab: null // result.url stays valid only while the result is alive

	// Grabs `item`, then calls `change()` and fades the old look out.
	function run(item, change) {
		const size = Qt.size(item.width * Screen.devicePixelRatio, item.height * Screen.devicePixelRatio)
		const grabbing = item.grabToImage(result => {
			grab = result
			source = result.url
			opacity = 1
			change()
			fade.restart()
		}, size)
		if (!grabbing)
			change()
	}

	anchors.fill: parent
	opacity: 0
	visible: opacity > 0

	NumberAnimation {
		id: fade
		target: shot
		property: "opacity"
		to: 0
		duration: Motion.slow
		easing.type: Motion.ease
		onFinished: {
			shot.source = ""
			shot.grab = null
		}
	}
}
