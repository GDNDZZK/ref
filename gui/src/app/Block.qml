// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Base of every ChatView row: centers its content in the reading column.
//
// A newly added row fades in and drops into place. The drop is a transform, not the
// row's y: ListView keeps moving the row while it enters, e.g. up as a reasoning
// block above folds away. An add Transition would pin the row to the destination
// it had when inserted and snap it to the real one only when done.
//
// While the block streams, its height chases the content height with a per-frame
// exponential ease, so new lines slide in instead of popping. A chase, not a
// Behavior: a Behavior restarts whenever its target changes, and content that is
// itself animating (a reasoning body easing open) changes it every frame, which
// keeps restarting the animation and stalls the height.
//
// A removed row stays in the view until it has faded out and its height has closed.
Item {
	id: block

	required property string content
	required property bool streaming
	required property bool stale // before the context window's start: the model no longer sees it
	required property int index

	signal added() // inserted into the model, as opposed to created with the view

	readonly property var chat: ListView.view // the ChatView; untyped, as ListView.view is a plain ListView to the linter
	readonly property real columnWidth: chat.columnWidth
	default property alias body: column.data

	property real pullUp: 0 // how far the content moves up into the gap above the row
	readonly property real naturalHeight: column.childrenRect.height - pullUp
	property real easedHeight: 0
	property bool easing: false // from the start of streaming until caught up after it ends

	width: chat.width
	height: easing ? easedHeight : naturalHeight
	clip: pullUp === 0
	transform: Translate { id: entryShift }

	ListView.onAdd: {
		if (chat.prepending) // earlier history, not news
			return
		entrance.restart()
		added()
	}

	// A removed row (undo, a rewritten history) fades out, then its space closes.
	ListView.onRemove: exit.start()

	function toggleExpanded() {
		chat.model.toggleExpanded(index)
	}

	// How far down the block a header `headerHeight` tall sits so that it stays in
	// view while the rest of the block scrolls past: pinned to the view's top, until
	// it meets the block's end. 0 while the block's top is in view.
	function pinOffset(headerHeight) {
		return Math.max(0, Math.min(chat.contentY - y, height - headerHeight))
	}

	// Folding the body under a pinned header would carry the header up out of view with
	// the block's end. While held, the view scrolls back as the block shrinks, so the
	// header stays where it is and the rows below close up under it. Release once the
	// fold is done: a hold left on would pull the view back on any later resize.
	property real heldHeaderHeight: 0 // the held header's height; 0 = none held

	function holdHeader(headerHeight) {
		heldHeaderHeight = headerHeight
	}

	function releaseHeader() {
		heldHeaderHeight = 0
	}

	onHeightChanged: {
		if (heldHeaderHeight === 0)
			return
		const pinLimit = y + height - heldHeaderHeight // furthest contentY that keeps the header pinned
		if (chat.contentY > pinLimit)
			chat.scrollTo(pinLimit, false)
	}

	Component.onCompleted: {
		easedHeight = naturalHeight
		easing = streaming
	}
	onStreamingChanged: {
		if (!streaming)
			return
		easedHeight = height
		easing = true
	}

	ParallelAnimation {
		id: entrance
		NumberAnimation { target: block; property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: entryShift; property: "y"; from: -Motion.enterDrop; to: 0; duration: Motion.standard; easing.type: Motion.ease }
	}

	SequentialAnimation {
		id: exit
		PropertyAction { target: block; property: "ListView.delayRemove"; value: true }
		NumberAnimation { target: block; property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
		NumberAnimation { target: block; property: "height"; to: 0; duration: Motion.standard; easing.type: Motion.ease }
		PropertyAction { target: block; property: "ListView.delayRemove"; value: false }
	}

	FrameAnimation {
		running: block.easing
		onTriggered: {
			block.easedHeight = Motion.chase(block.easedHeight, block.naturalHeight, frameTime)
			block.easing = block.streaming || !Motion.arrived(block.easedHeight, block.naturalHeight)
		}
	}

	Item {
		id: column
		x: (block.width - block.columnWidth) / 2
		y: -block.pullUp
		width: block.columnWidth
		opacity: block.stale ? Theme.staleOpacity : 1

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}
}
