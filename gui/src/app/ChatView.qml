import QtQuick
import QtQuick.Controls.Basic
import QtQml.Models
import Qutie

// The conversation, one delegate per block, centered in a reading column.
//
// Following: while `following`, contentY chases the end with an exponential ease
// instead of jumping. Only user gestures change `following`: scrolling (wheel,
// touchpad, touch) or dragging the scroll bar releases it, and ending such a gesture
// at the end re-engages it.
//
// Holding: while `turnInProgress` and following, the end never moves up. Streaming
// content can briefly take more room than it ends up needing (a line that wraps,
// then fits); the footer grows by what the content gave back, so the view does not
// bounce. When the turn ends, the held room is released with an animation.
//
// Wheel: each notch moves `wheelTarget` and contentY chases it, so fast notches add
// up. Flickable's own wheel animation restarts from wherever the previous notch got
// to and drops the rest of its distance. Touchpad deltas are already continuous and
// move the content directly.
//
// Both chases (following, wheel) step in whole pixels; see scrollStep().
//
// Geometry: every row stays instantiated (cacheBuffer), so contentHeight and originY
// are exact. Once rows off screen are destroyed, ListView estimates them from the
// average row height, and a tall code block entering or leaving the view resizes the
// scroll bar and shifts the content under a dragged handle.
ListView {
	id: view

	readonly property real columnWidth: Math.min(width - 2 * Theme.pagePadding, Theme.contentWidth)
	property bool following: true
	property bool turnInProgress: false

	signal errorOutOfSight() // an error block arrived while the user was reading further up

	// Where the content ends, without and with the held room. The footer sits right
	// after the last row.
	readonly property real naturalEnd: footerItem ? footerItem.y + Theme.pagePadding : 0
	readonly property real heldEnd: naturalEnd + heldSpace
	property real highWater: 0 // furthest naturalEnd of the current turn
	property real heldSpace: 0
	property bool holdUpdating: false // holdEnd() is writing heldSpace
	property bool holdStale: false // the end moved during that write

	readonly property real minContentY: originY - topMargin
	readonly property real maxContentY: Math.max(minContentY, originY + contentHeight - height)

	// Flickable clamps contentY to this bound; aiming past it would chase forever.
	readonly property real boundedEnd: Math.min(heldEnd, originY + contentHeight)
	readonly property real followTarget: Math.max(minContentY, boundedEnd - height)

	property bool wheeling: false // chasing wheelTarget; never while following
	property real wheelTarget: 0
	readonly property int wheelNotch: 120 // angleDelta of one notch, per QWheelEvent
	readonly property real wheelStep: Application.styleHints.wheelScrollLines * 24 // px per notch, as in Flickable

	// The follow chase below does the gliding.
	function glideToEnd() {
		following = true
	}

	// One frame of a scroll chase toward `target`, in whole pixels and at least one per
	// frame. Every ListView layout fixes up the position, which rounds a fractional
	// contentY (qquickflickable.cpp:537-549). While a row resizes every frame, that
	// rounding undid sub-pixel steps: the chase stalled a few pixels short and only
	// finished once the rows settled, as a small late scroll.
	function scrollStep(target, frameTime) {
		const from = Math.round(contentY)
		const next = Math.round(Motion.chase(contentY, target, frameTime))
		const step = next !== from ? next : from + Math.sign(target - from)
		const overshoots = (step - target) * (from - target) < 0 // ends up past a fractional target
		contentY = overshoots ? target : step
	}

	// Within a pixel of the target counts as there: scrolling moves in whole pixels.
	function scrollArrived(target) {
		return Math.abs(target - contentY) < 1
	}

	// A user scroll to `y`: `glide` chases it (wheel notches), otherwise it applies at
	// once (touchpad deltas). Reaching the end hands over to following, which keeps up
	// with new content.
	function scrollTo(y, glide) {
		scrollActivity.restart()
		const target = Math.max(minContentY, Math.min(maxContentY, y))
		if (target === maxContentY) {
			following = true
			return
		}
		following = false
		if (glide) {
			wheelTarget = target
			wheeling = true
			return
		}
		wheeling = false
		contentY = target
	}

	// Resizing the footer makes ListView place it again at once, from the rows' current
	// heights; if those changed since the last layout, the end moves while heldSpace is
	// being written. Recursing from there would re-enter the footer's height binding, so
	// a move during the update only marks it stale and the update runs another pass.
	// The footer's height does not move the footer, so the second pass settles.
	function holdEnd() {
		if (holdUpdating) {
			holdStale = true
			return
		}
		holdUpdating = true
		do {
			holdStale = false
			highWater = Math.max(highWater, naturalEnd)
			heldSpace = highWater - naturalEnd
		} while (holdStale)
		holdUpdating = false
	}

	clip: true
	spacing: Theme.blockSpacing
	topMargin: Theme.pagePadding
	cacheBuffer: 1e7 // px, beyond any conversation: keeps every row instantiated
	boundsBehavior: Flickable.StopAtBounds
	acceptedButtons: Qt.NoButton // mouse drags select text; wheel, touchpad and touch still scroll

	// Bottom padding plus held room. A footer rather than bottomMargin, so its
	// position marks the exact end of the rows.
	footer: Item {
		width: view.width
		height: Theme.pagePadding + view.heldSpace
	}

	ScrollBar.vertical: ScrollBar {
		id: scrollBar

		onPressedChanged: {
			view.wheeling = false
			view.following = !pressed && view.atYEnd
		}

		// Basic's handle takes the system palette; this one follows Theme.
		contentItem: Rectangle {
			id: handle
			implicitWidth: 6
			radius: width / 2
			color: scrollBar.pressed ? Theme.scrollHandlePressed : Theme.scrollHandle
			opacity: 0

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			states: State {
				name: "shown"
				when: (scrollBar.active || scrollActivity.running) && scrollBar.size < 1
				PropertyChanges { handle.opacity: 1 }
			}
			transitions: [
				Transition {
					to: "shown"
					NumberAnimation { target: handle; property: "opacity"; duration: Motion.quick; easing.type: Motion.ease }
				},
				Transition {
					from: "shown"
					SequentialAnimation {
						PauseAnimation { duration: Motion.slow }
						NumberAnimation { target: handle; property: "opacity"; duration: Motion.standard; easing.type: Motion.ease }
					}
				}
			]
		}
	}

	// Takes every wheel event and tells wheel from touchpad by the deltas, not the
	// device: on Wayland, Qt reports a mouse wheel as the seat's "touchpad" device.
	WheelHandler {
		acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
		onWheel: event => {
			// Like Flickable: a scroll phase means a touchpad gesture, whose pixel deltas
			// follow the fingers. Its Begin and End carry no delta.
			const gesture = event.phase !== Qt.NoScrollPhase
			if (gesture) {
				if (event.pixelDelta.y !== 0)
					view.scrollTo(view.contentY - event.pixelDelta.y, false)
				return
			}
			if (event.angleDelta.y === 0) // horizontal only
				return
			const start = view.wheeling ? view.wheelTarget : view.contentY
			view.scrollTo(start - event.angleDelta.y / view.wheelNotch * view.wheelStep, true)
		}
	}

	// Keeps the scroll bar shown while wheel or touchpad scrolling goes on; setting
	// contentY directly does not make Flickable report movement.
	Timer {
		id: scrollActivity
		interval: Motion.standard
	}

	FrameAnimation {
		running: view.wheeling
		onTriggered: {
			view.scrollStep(view.wheelTarget, frameTime)
			view.wheeling = !view.scrollArrived(view.wheelTarget)
		}
	}

	onMovementStarted: {
		wheeling = false
		following = false
	}
	onMovementEnded: following = atYEnd

	// Hold only while following: further up, the end moving is out of sight.
	onNaturalEndChanged: if (turnInProgress && following) holdEnd()
	onFollowingChanged: {
		if (!following)
			return
		wheeling = false
		highWater = heldEnd
	}
	onTurnInProgressChanged: {
		if (turnInProgress) {
			release.stop()
			highWater = heldEnd // a new turn keeps whatever is still held
			holdEnd()
		} else {
			release.restart()
		}
	}

	NumberAnimation {
		id: release
		target: view
		property: "heldSpace"
		to: 0
		duration: Motion.slow
		easing.type: Motion.ease
	}

	FrameAnimation {
		running: view.following && !view.scrollArrived(view.followTarget)
		onTriggered: view.scrollStep(view.followTarget, frameTime)
	}

	// Rows animate their own entrance (Block.qml).
	delegate: DelegateChooser {
		role: "kind"

		DelegateChoice { roleValue: "user"; UserBlock {} }
		DelegateChoice { roleValue: "reasoning"; ReasoningBlock {} }
		DelegateChoice { roleValue: "markdown"; MarkdownBlock {} }
		DelegateChoice { roleValue: "code"; CodeBlock {} }
		DelegateChoice { roleValue: "tool"; ToolBlock {} }
		DelegateChoice { roleValue: "error"; ErrorBlock {} }
	}
}
