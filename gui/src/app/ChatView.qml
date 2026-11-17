// Adapted from an early version of LightBlueCube/qutie
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQml.Models
import RefGui

// The conversation, one delegate per block, centered in a reading column.
//
// Following: while `following`, contentY chases the end with an exponential ease
// instead of jumping. Only user gestures change `following`: scrolling (wheel,
// touchpad, touch) or dragging the scroll bar releases it, and ending such a gesture
// at the end re-engages it.
//
// Wheel: each notch moves `wheelTarget` and contentY chases it, so fast notches add
// up. Flickable's own wheel animation restarts from wherever the previous notch got
// to and drops the rest of its distance. Touchpad deltas are already continuous and
// move the content directly.
//
// Both chases (following, wheel) step in whole pixels; see scrollStep().
//
// Geometry: the rows within `retainedDistance` of the view stay instantiated, so
// contentHeight and originY are exact for all but very long conversations. Beyond that,
// ListView estimates the rows it destroyed from the average row height, and a tall code
// block entering or leaving the view resizes the scroll bar and shifts the content under a
// dragged handle. Keeping every row costs memory: about 0.25 MB per row measured with a
// mock session of 800 rows, 256 MB against 124 MB with the default cache.
ListView {
	id: view

	property real sidePadding: Theme.pagePadding // narrower in a split-view tile
	property real layoutWidth: width
	readonly property real columnWidth: Math.max(0, Math.min(layoutWidth - 2 * sidePadding, Theme.contentWidth))
	property bool following: true
	property real previousHeight: 0
	property string sessionId // the session whose pictures the rows show
	property bool moreBefore: false // the backend has earlier messages
	property bool loadingOlder: false
	property string olderError
	readonly property bool prepending: loadingOlder // rows added now are earlier history
	property int permissionCount: 0 // tool calls waiting for the user's answer

	signal errorOutOfSight() // an error block arrived while the user was reading further up
	signal undoRequested(string messageId)
	signal copyRequested(string messageId)
	signal loadOlderRequested()
	signal imageClicked(var image) // a thumbnail, to be shown large
	signal cancelRequested() // the Cancel button of a delayed tool call
	signal permissionAnswered(string callId, bool allow, bool always) // from a tool card that asks for permission

	// Where the content ends. The footer sits right after the last row.
	readonly property real naturalEnd: footerItem ? footerItem.y + Theme.pagePadding : 0
	property bool compact: false // narrower cache in a split-view tile
	readonly property int fullRetainedDistance: 20000 // px above and below the view whose rows stay instantiated, about 200 rows
	readonly property int compactRetainedDistance: 1600 // smaller cache in split-view tiles so opening many tiles stays fast
	readonly property int retainedDistance: compact ? compactRetainedDistance : fullRetainedDistance
	readonly property int olderTriggerDistance: 400 // px from the top at which the next page loads by itself
	readonly property real minContentY: originY - topMargin
	readonly property real maxContentY: Math.max(minContentY, originY + contentHeight - height)

	// Flickable clamps contentY to this bound; aiming past it would chase forever.
	readonly property real boundedEnd: Math.min(naturalEnd, originY + contentHeight)
	readonly property real followTarget: Math.max(minContentY, boundedEnd - height)

	property bool wheeling: false // chasing wheelTarget; never while following
	property real wheelTarget: 0
	readonly property int wheelNotch: 120 // angleDelta of one notch, per QWheelEvent
	readonly property int wheelPixelsPerLine: 40
	readonly property int minWheelStep: 120
	readonly property real wheelStep: Math.max(minWheelStep, Application.styleHints.wheelScrollLines * wheelPixelsPerLine)
	readonly property real wheelLag: 115 // ms; exponential time constant for wheel glide
	readonly property int wheelBoostWindowMs: 200 // consecutive notches inside this window build speed
	readonly property real wheelBoostStep: 0.4
	readonly property real wheelMaxBoost: 3.0
	property real wheelBoost: 1.0
	property real lastWheelTime: 0
	property int lastWheelDirection: 0

	property bool coasting: false
	property real coastVelocity: 0 // px/s; positive scrolls down
	property real coastRemainder: 0 // sub-pixel accumulator for whole-pixel contentY steps
	property real touchVelocity: 0 // smoothed px/s during ScrollUpdate
	property real pendingTouchDelta: 0
	property real lastTouchTime: 0
	readonly property real touchVelocityAlpha: 0.45
	readonly property int touchMinSampleMs: 4
	readonly property int touchSampleTimeoutMs: 120
	readonly property real coastStartMinVelocity: 60 // px/s minimum flick velocity to start coasting
	readonly property real coastStopVelocity: 15 // px/s below which coasting stops
	readonly property real coastDecay: 0.32 // s; exponential decay time constant
	// The follow chase below does the gliding.
	function glideToEnd() {
		following = true
	}

	// One frame of a scroll chase toward `target`, in whole pixels and at least one per
	// frame. Every ListView layout fixes up the position, which rounds a fractional
	// contentY (qquickflickable.cpp:537-549). While a row resizes every frame, that
	// rounding undid sub-pixel steps: the chase stalled a few pixels short and only
	// finished once the rows settled, as a small late scroll.
	function wheelChase(current, target, frameTime) {
		if (Motion.arrived(current, target))
			return target
		return current + (target - current) * (1 - Math.exp(-frameTime * Format.msPerSecond / wheelLag))
	}

	// One frame of a scroll chase toward `target`, in whole pixels and at least one per
	// frame. Every ListView layout fixes up the position, which rounds a fractional
	// contentY (qquickflickable.cpp:537-549). While a row resizes every frame, that
	// rounding undid sub-pixel steps: the chase stalled a few pixels short and only
	// finished once the rows settled, as a small late scroll.
	function scrollStep(target, frameTime, forWheel) {
		const from = Math.round(contentY)
		const raw = forWheel ? wheelChase(contentY, target, frameTime) : Motion.chase(contentY, target, frameTime)
		const next = Math.round(raw)
		const step = next !== from ? next : from + Math.sign(target - from)
		const overshoots = (step - target) * (from - target) < 0 // ends up past a fractional target
		contentY = overshoots ? target : step
	}

	// Within a pixel of the target counts as there: scrolling moves in whole pixels.
	function scrollArrived(target) {
		return Math.abs(target - contentY) < 1
	}

	function stopCoast() {
		coasting = false
		coastVelocity = 0
		coastRemainder = 0
	}

	function resetTouchTracker() {
		touchVelocity = 0
		pendingTouchDelta = 0
		lastTouchTime = Date.now()
	}

	function sampleTouchVelocity(deltaY, now) {
		const dtMs = lastTouchTime > 0 ? now - lastTouchTime : 0
		if (lastTouchTime === 0 || dtMs > touchSampleTimeoutMs) {
			touchVelocity = 0
			pendingTouchDelta = deltaY
			lastTouchTime = now
			return
		}
		pendingTouchDelta += deltaY
		if (dtMs < touchMinSampleMs)
			return
		const instant = pendingTouchDelta / (dtMs / Format.msPerSecond)
		touchVelocity = touchVelocity === 0
			? instant
			: touchVelocity + touchVelocityAlpha * (instant - touchVelocity)
		pendingTouchDelta = 0
		lastTouchTime = now
	}

	function startCoast() {
		const now = Date.now()
		if (lastTouchTime > 0 && now - lastTouchTime > touchSampleTimeoutMs) {
			touchVelocity = 0
			pendingTouchDelta = 0
			return
		}
		if (pendingTouchDelta !== 0 && lastTouchTime > 0 && now > lastTouchTime) {
			const dtMs = Math.max(touchMinSampleMs, now - lastTouchTime)
			const instant = pendingTouchDelta / (dtMs / Format.msPerSecond)
			touchVelocity = touchVelocity === 0
				? instant
				: touchVelocity + touchVelocityAlpha * (instant - touchVelocity)
			pendingTouchDelta = 0
		}
		const velocity = touchVelocity
		touchVelocity = 0
		pendingTouchDelta = 0
		if (Math.abs(velocity) <= coastStartMinVelocity)
			return
		if (velocity < 0 && contentY <= minContentY)
			return
		if (velocity > 0 && (contentY >= maxContentY || following))
			return
		following = false
		wheeling = false
		coastVelocity = velocity
		coastRemainder = 0
		coasting = true
		scrollActivity.restart()
	}

	function coastStep(frameTime) {
		scrollActivity.restart()
		const prevVelocity = coastVelocity
		const decay = Math.exp(-frameTime / coastDecay)
		coastVelocity = prevVelocity * decay
		const distance = prevVelocity * coastDecay * (1 - decay)
		const total = coastRemainder + distance
		const step = Math.trunc(total)
		coastRemainder = total - step
		if (step !== 0) {
			const next = Math.max(minContentY, Math.min(maxContentY, contentY + step))
			contentY = next
			if (next <= minContentY && coastVelocity < 0) {
				stopCoast()
				return
			}
			if (next >= maxContentY && coastVelocity > 0) {
				stopCoast()
				following = true
				return
			}
		}
		if (Math.abs(coastVelocity) < coastStopVelocity) {
			stopCoast()
			if (contentY >= maxContentY)
				following = true
		}
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

	function stopScrollMotion() {
		wheeling = false
		wheelBoost = 1.0
		lastWheelTime = 0
		lastWheelDirection = 0
		stopCoast()
		touchVelocity = 0
		pendingTouchDelta = 0
		lastTouchTime = 0
	}
	clip: true
	spacing: Theme.blockSpacing
	topMargin: Theme.pagePadding
	cacheBuffer: retainedDistance
	boundsBehavior: Flickable.StopAtBounds
	acceptedButtons: Qt.NoButton // mouse drags select text; wheel, touchpad and touch still scroll

	// Whether earlier messages could be asked for now. A failed load waits for a click.
	readonly property bool canLoadOlder: moreBefore && count > 0 && !loadingOlder && olderError === ""

	// Loads earlier messages when the top is near.
	function loadOlderIfNear() {
		const nearTop = contentY - originY < olderTriggerDistance
		if (canLoadOlder && nearTop && !following)
			loadOlderRequested()
	}

	// The view keeps the rows it shows where they are when earlier ones are added above them.
	// The end stays on the bottom edge when the view changes height (a tile's input box comes or goes).
	onSessionIdChanged: stopScrollMotion()
	Connections {
		target: view.model ? view.model : null
		function onModelReset() {
			view.stopScrollMotion()
		}
	}
	onHeightChanged: {
		if (following && previousHeight > 0)
			contentY -= height - previousHeight
		previousHeight = height
	}
	onContentYChanged: loadOlderIfNear()
	onMoreBeforeChanged: loadOlderIfNear()
	onLoadingOlderChanged: loadOlderIfNear()

	header: Item {
		width: view.width
		height: view.moreBefore ? pill.height + Theme.blockSpacing : 0
		clip: true

		OlderPill {
			id: pill
			width: parent.width
			loading: view.loadingOlder
			error: view.olderError
			onClicked: view.loadOlderRequested()
		}
	}

	// Bottom padding. A footer rather than bottomMargin, so its position marks the
	// exact end of the rows.
	footer: Item {
		width: view.width
		height: Theme.pagePadding
	}

	ScrollBar.vertical: ScrollBar {
		id: scrollBar

		onPressedChanged: {
			view.wheeling = false
			view.stopCoast()
			view.following = !pressed && view.atYEnd
		}

		// Basic's handle takes the system palette; this one follows Theme.
		contentItem: ScrollHandle {
			id: handle
			implicitWidth: thickness
			pressed: scrollBar.pressed
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
				switch (event.phase) {
				case Qt.ScrollBegin:
					view.stopCoast()
					view.wheeling = false
					view.resetTouchTracker()
					if (event.pixelDelta.y !== 0)
						view.scrollTo(view.contentY - event.pixelDelta.y, false)
					return
				case Qt.ScrollUpdate:
					if (event.pixelDelta.y !== 0) {
						const deltaY = -event.pixelDelta.y
						// Fingers moving against the coast stop it at once, also when the platform
						// sent no ScrollBegin for this gesture.
						if (view.coasting && deltaY * view.coastVelocity < 0)
							view.stopCoast()
						view.scrollTo(view.contentY + deltaY, false)
						view.sampleTouchVelocity(deltaY, Date.now())
					}
					return
				case Qt.ScrollMomentum:
					view.stopCoast()
					view.touchVelocity = 0
					view.pendingTouchDelta = 0
					if (event.pixelDelta.y !== 0)
						view.scrollTo(view.contentY - event.pixelDelta.y, false)
					return
				case Qt.ScrollEnd:
					if (event.pixelDelta.y !== 0) {
						const deltaY = -event.pixelDelta.y
						view.scrollTo(view.contentY + deltaY, false)
						view.sampleTouchVelocity(deltaY, Date.now())
					}
					view.startCoast()
					return
				}
				return
			}
			if (event.angleDelta.y === 0) // horizontal only
				return
			view.stopCoast()
			const now = Date.now()
			const dir = Math.sign(event.angleDelta.y)
			if (dir === view.lastWheelDirection && view.lastWheelTime > 0 && now - view.lastWheelTime <= view.wheelBoostWindowMs) {
				const notchShare = Math.min(1, Math.abs(event.angleDelta.y) / view.wheelNotch)
				view.wheelBoost = Math.min(view.wheelMaxBoost, view.wheelBoost + view.wheelBoostStep * notchShare)
			} else {
				view.wheelBoost = 1.0
			}
			view.lastWheelTime = now
			// A notch against the running chase starts from where the content is: the remaining
			// old-direction distance is dropped, so the view turns around at once.
			const reversed = dir !== view.lastWheelDirection
			view.lastWheelDirection = dir
			const start = view.wheeling && !reversed ? view.wheelTarget : view.contentY
			view.scrollTo(start - event.angleDelta.y / view.wheelNotch * view.wheelStep * view.wheelBoost, true)
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
			const target = Math.max(view.minContentY, Math.min(view.maxContentY, view.wheelTarget))
			view.wheelTarget = target
			view.scrollStep(target, frameTime, true)
			view.wheeling = !view.scrollArrived(target)
		}
	}

	FrameAnimation {
		running: view.coasting
		onTriggered: view.coastStep(frameTime)
	}

	onMovementStarted: {
		wheeling = false
		stopCoast()
		following = false
	}
	onMovementEnded: following = atYEnd

	onFollowingChanged: {
		if (!following)
			return
		wheeling = false
		stopCoast()
	}
	FrameAnimation {
		running: view.following && !view.scrollArrived(view.followTarget)
		onTriggered: view.scrollStep(view.followTarget, frameTime)
	}

	// Rows animate their own entrance (Block.qml).
	delegate: DelegateChooser {
		role: "kind"

		DelegateChoice { roleValue: "user"; UserBlock {} }
		DelegateChoice { roleValue: "image"; ImageBlock {} }
		DelegateChoice { roleValue: "reasoning"; ReasoningBlock {} }
		DelegateChoice { roleValue: "markdown"; MarkdownBlock {} }
		DelegateChoice { roleValue: "code"; CodeBlock {} }
		DelegateChoice { roleValue: "tool"; ToolBlock {} }
		DelegateChoice { roleValue: "error"; ErrorBlock {} }
		DelegateChoice { roleValue: "meta"; MetaBlock {} }
	}
}
