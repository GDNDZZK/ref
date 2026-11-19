import QtQuick
import RefGui

// Changes something under a snapshot of how it looked and fades the snapshot out, so that
// everything on screen crossfades at once without animating each item.
Image {
	id: shot

	property var grab: null // result.url stays valid only while the result is alive
	property int serial: 0

	function cancel() {
		++serial
		fade.stop()
		opacity = 0
		source = ""
		grab = null
	}

	// Grabs `item`, then calls `change()` and fades the old look out.
	function run(item, change) {
		const token = ++serial
		if (!item || item.width <= 0 || item.height <= 0) {
			fade.stop()
			opacity = 0
			source = ""
			grab = null
			change()
			return
		}
		const scale = Workspace.settings.uiScale
		const size = Qt.size(Math.max(1, Math.round(item.width * scale)), Math.max(1, Math.round(item.height * scale)))
		const grabbing = item.grabToImage(result => {
			if (token !== shot.serial)
				return
			grab = result
			source = result.url
			opacity = 1
			change()
			fade.restart()
		}, size)
		if (!grabbing) {
			fade.stop()
			opacity = 0
			source = ""
			grab = null
			change()
		}
	}

	anchors.fill: parent
	opacity: 0
	visible: opacity > 0

	NumberAnimation {
		id: fade
		target: shot
		property: "opacity"
		to: 0
		duration: Motion.standard
		easing.type: Motion.ease
		onFinished: {
			shot.source = ""
			shot.grab = null
		}
	}
}
