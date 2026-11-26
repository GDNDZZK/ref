import QtQuick
import RefGui

// Changes something under a snapshot of how it looked and either fades the snapshot out (`run`)
// or reveals the new look via a circular wave around `origin` (`runRadial`).
Item {
	id: shot

	property var grab: null // result.url stays valid only while the result is alive
	property int serial: 0
	property bool radialActive: false
	property bool radialExpand: true
	readonly property bool holdingLightToggle: radialActive && !radialExpand && revealItem.radius > 17

	function cancel() {
		++serial
		fade.stop()
		radialAnim.stop()
		radialActive = false
		fadeImage.opacity = 0
		fadeImage.source = ""
		revealItem.clear()
		grab = null
	}

	// Grabs `item`, then calls `change()` and fades the old look out.
	function run(item, change) {
		const token = ++serial
		radialAnim.stop()
		radialActive = false
		revealItem.clear()
		if (!item || item.width <= 0 || item.height <= 0) {
			fade.stop()
			fadeImage.opacity = 0
			fadeImage.source = ""
			grab = null
			change()
			return
		}
		const scale = UiScale.factor * (Screen.devicePixelRatio || 1)
		const size = Qt.size(Math.max(1, Math.round(item.width * scale)), Math.max(1, Math.round(item.height * scale)))
		const grabbing = item.grabToImage(result => {
			if (token !== shot.serial)
				return
			grab = result
			fadeImage.source = result.url
			fadeImage.opacity = 1
			change()
			fade.restart()
		}, size)
		if (!grabbing) {
			fade.stop()
			fadeImage.opacity = 0
			fadeImage.source = ""
			grab = null
			change()
		}
	}

	// Grabs `item`, then reveals the new look with a circular wave centered at `origin`.
	// `expand === true` (dark -> light): expanding hole reveals new light theme outward from `origin`.
	// `expand === false` (light -> dark): old light snapshot shrinks radially back into `origin`.
	function runRadial(item, origin, expand, change) {
		const token = ++serial
		fade.stop()
		fadeImage.opacity = 0
		fadeImage.source = ""
		radialAnim.stop()
		radialActive = false
		if (!item || item.width <= 0 || item.height <= 0) {
			revealItem.clear()
			grab = null
			change()
			return
		}
		const scale = UiScale.factor * (Screen.devicePixelRatio || 1)
		const size = Qt.size(Math.max(1, Math.round(item.width * scale)), Math.max(1, Math.round(item.height * scale)))
		const grabbing = item.grabToImage(result => {
			if (token !== shot.serial)
				return
			grab = result
			const cx = origin ? origin.x : shot.width * 0.5
			const cy = origin ? origin.y : shot.height * 0.5
			const dx = Math.max(Math.abs(cx), Math.abs(shot.width - cx))
			const dy = Math.max(Math.abs(cy), Math.abs(shot.height - cy))
			const maxRadius = Math.hypot(dx, dy) + 4.0
			const startRadius = expand ? 17.0 : maxRadius
			revealItem.center = Qt.point(cx, cy)
			revealItem.inverted = expand
			revealItem.radius = startRadius
			revealItem.setGrabResult(result)
			shot.radialExpand = expand
			shot.radialActive = true
			change()
			radialDelay.duration = expand ? 105 : 0
			radialRadiusAnim.from = startRadius
			radialRadiusAnim.to = expand ? maxRadius : 0.0
			radialRadiusAnim.easing.type = expand ? Easing.OutCubic : Easing.InOutCubic
			radialAnim.restart()
		}, size)
		if (!grabbing) {
			shot.radialActive = false
			revealItem.clear()
			grab = null
			change()
		}
	}

	anchors.fill: parent
	visible: fadeImage.opacity > 0 || shot.radialActive
	Image {
		id: fadeImage
		anchors.fill: parent
		opacity: 0
		visible: opacity > 0
	}

	CircularRevealItem {
		id: revealItem
		anchors.fill: parent
		visible: shot.radialActive
	}

	NumberAnimation {
		id: fade
		target: fadeImage
		property: "opacity"
		to: 0
		duration: Motion.standard
		easing.type: Motion.ease
		onFinished: {
			fadeImage.source = ""
			shot.grab = null
		}
	}

	SequentialAnimation {
		id: radialAnim
		onFinished: {
			shot.radialActive = false
			revealItem.clear()
			shot.grab = null
		}
		// Dark -> Light waits 105ms (sunriseProgress = 0.18) so the daylight wave erupts right as the sun rays burst outward.
		PauseAnimation {
			id: radialDelay
			duration: 0
		}
		NumberAnimation {
			id: radialRadiusAnim
			target: revealItem
			property: "radius"
			duration: 460
			easing.type: Easing.InOutCubic
		}
	}
}
