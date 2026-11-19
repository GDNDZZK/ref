import QtQuick
import RefGui

// A running session in the sidebar: state dot, title, directory and run time, to-do progress.
// Permission requests and failed turns stand out.
Item {
	id: row

	required property string sessionId
	required property string displayTitle
	required property string name
	required property string dirName
	required property string activity
	required property string lastError
	required property bool hasError
	required property bool hasPermission
	required property double busySince
	required property int todoDone
	required property int todoTotal
	required property bool todoAllDone
	required property bool isCurrent
	required property bool isTile
	required property int index

	property bool renaming: false
	property bool anyDragging: false
	property bool dragging: false
	property real dragCardY: 0
	property bool settlingDrag: false

	signal dragActiveChanged(string sessionId, bool active)
	signal reorderRequested(string sessionId, int toIndex)
	readonly property bool working: Activity.isWorking(activity)
	readonly property bool failed: Activity.isFailed(activity, hasError)
	readonly property bool retrying: activity === Activity.retrying
	readonly property bool attention: hasPermission || failed
	readonly property bool hovered: hover.hovered
	readonly property bool showCloseButton: hover.hovered && !row.anyDragging && !row.renaming
	readonly property bool canDetach: isCurrent && !Workspace.splitView
	readonly property int renameEntry: 0 // index in the context menu
	// What the line under the title says.
	readonly property string subtitle: {
		if (activity === Activity.waitingQuestion)
			return qsTr("Waiting for your answer")
		if (hasPermission)
			return qsTr("Needs permission")
		if (failed)
			return lastError.split("\n")[0]
		if (retrying)
			return qsTr("Retrying")
		if (activity === Activity.compacting)
			return qsTr("Compacting")
		return busySince > 0 ? dirName + " · " + Format.elapsed(ticker.now - busySince) : dirName
	}
	readonly property color faceColor: {
		if (hasPermission)
			return Theme.warningSurface
		if (isCurrent)
			return Theme.userBubble
		return hover.hovered ? Theme.surfaceHover : "transparent"
	}
	readonly property color borderColor: {
		if (hasPermission)
			return Theme.warningBorder
		return isCurrent ? Theme.userBubbleBorder : "transparent"
	}

	function startRename() {
		renaming = true
		renameInput.text = name !== "" ? name : displayTitle
		renameInput.forceActiveFocus()
		renameInput.selectAll()
	}

	function commitRename() {
		if (!renaming)
			return
		renaming = false
		if (renameInput.text.trim() !== name)
			Workspace.renameSession(sessionId, renameInput.text.trim())
	}

	width: ListView.view ? ListView.view.width : 0
	height: Theme.sidebarRowHeight
	z: row.dragging ? 10 : 0
	onYChanged: {
		if (dragging)
			dragCardY = dragArea.dragVisualTop - y
	}

	Ticker {
		id: ticker
		interval: Format.msPerSecond
		running: row.busySince > 0 && row.visible
	}

	Rectangle {
		id: card
		x: Theme.sidebarPadding
		y: row.dragging ? row.dragCardY : 0
		width: parent.width - 2 * Theme.sidebarPadding
		height: parent.height
		radius: Theme.radius
		color: row.faceColor
		border.color: row.dragging ? Theme.accentText : row.borderColor
		scale: (dragArea.pressed && !row.dragging) ? Motion.pressedScaleRow : 1

		Behavior on y {
			enabled: row.settlingDrag
			NumberAnimation {
				duration: Motion.quick
				easing.type: Motion.ease
				onFinished: row.settlingDrag = false
			}
		}
		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		// Marks the sessions shown as tiles in split view.
		Rectangle {
			width: 2
			height: parent.height - 2 * 10
			anchors.verticalCenter: parent.verticalCenter
			radius: 1
			color: Theme.accent
			opacity: row.isTile ? 1 : 0

			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		SessionDot {
			x: 12
			y: 14
			activity: row.activity
			hasError: row.hasError
		}

		Text {
			textFormat: Text.PlainText
			id: title
			x: 30
			y: 8
			width: tail.x - x - 8
			height: 20
			verticalAlignment: Text.AlignVCenter
			text: row.displayTitle
			color: Theme.text
			font.pixelSize: Theme.compactFontSize
			elide: Text.ElideRight
			visible: !row.renaming
		}

		TextInput {
			id: renameInput
			x: 30
			y: 8
			width: tail.x - x - 8
			height: 20
			visible: row.renaming
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.compactFontSize
			clip: true
			onAccepted: row.commitRename()
			onActiveFocusChanged: if (!activeFocus) row.commitRename()
			Keys.onEscapePressed: row.renaming = false
		}

		Text {
			textFormat: Text.PlainText
			x: 30
			y: 30
			width: parent.width - x - 12
			text: row.subtitle
			color: row.hasPermission || row.retrying ? Theme.warningText : row.failed ? Theme.dangerText : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			elide: Text.ElideRight
		}

		// Right side: the mark that needs attention, else the to-do progress; the close button on hover.
		Item {
			id: tail
			x: parent.width - width - 12
			y: 8
			width: 22
			height: 20

			Row {
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				spacing: 4
				opacity: row.showCloseButton ? 0 : 1

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: row.activity === Activity.waitingQuestion ? "question" : "lock"
					size: 16
					color: Theme.warningText
					visible: row.hasPermission
				}
				AlertBadge {
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: 14
					implicitHeight: 14
					visible: row.failed && !row.hasPermission
				}
				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: "check"
					size: 16
					color: Theme.successText
					visible: row.todoAllDone && !row.attention
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					text: row.todoDone + "/" + row.todoTotal
					color: row.todoAllDone ? Theme.successText : Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					visible: row.todoTotal > 0 && !row.attention
				}
			}

			IconButton {
				id: closeButton
				anchors.centerIn: parent
				buttonSize: 22
				iconName: "close"
				iconSize: 16
				opacity: row.showCloseButton ? 1 : 0
				visible: opacity > 0
				enabled: !row.anyDragging && !row.renaming
				Accessible.name: qsTr("Close session")
				onClicked: Workspace.requestCloseSession(row.sessionId)

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	HoverHandler {
		id: hover
		enabled: !row.renaming && (!row.anyDragging || row.dragging)
		cursorShape: row.dragging ? Qt.ClosedHandCursor : Qt.PointingHandCursor
	}
	MouseArea {
		id: dragArea
		anchors.fill: parent
		acceptedButtons: Qt.LeftButton
		enabled: !row.renaming
		preventStealing: row.dragging
		cursorShape: row.dragging ? Qt.ClosedHandCursor : Qt.PointingHandCursor

		property real pressX: 0
		property real pressY: 0
		property real pressOffsetY: 0
		property real dragVisualTop: 0
		property real lastSceneX: 0
		property real lastSceneY: 0
		property bool movedBeforeHold: false
		property bool wasDragging: false
		property int scrollDirection: 0
		readonly property int holdDelayMs: 280
		readonly property real moveThreshold: 6
		readonly property real autoScrollMargin: 24
		readonly property real autoScrollStep: 4

		function updateDrag() {
			const view = row.ListView.view
			if (!row.dragging || !view)
				return
			if (view.count <= 1) {
				finishDrag(false)
				return
			}
			const contentPt = view.contentItem.mapFromItem(null, lastSceneX, lastSceneY)
			const maxTop = Math.max(0, view.contentHeight - row.height)
			const relY = Math.max(0, Math.min(maxTop, contentPt.y - view.originY - pressOffsetY))
			dragVisualTop = view.originY + relY
			row.dragCardY = dragVisualTop - row.y
			const stride = Theme.sidebarRowHeight + view.spacing
			const targetIndex = stride > 0
				? Math.max(0, Math.min(view.count - 1, Math.round(relY / stride)))
				: row.index
			if (targetIndex !== row.index)
				row.reorderRequested(row.sessionId, targetIndex)

			if (view.contentHeight > view.height) {
				const minY = view.originY
				const maxY = view.originY + Math.max(0, view.contentHeight - view.height)
				const viewPt = view.mapFromItem(null, lastSceneX, lastSceneY)
				if (viewPt.y < autoScrollMargin && view.contentY > minY)
					scrollDirection = -1
				else if (viewPt.y > view.height - autoScrollMargin && view.contentY < maxY)
					scrollDirection = 1
				else
					scrollDirection = 0
			} else {
				scrollDirection = 0
			}
		}

		function finishDrag(animateSettle) {
			holdTimer.stop()
			scrollDirection = 0
			if (!row.dragging)
				return
			if (animateSettle && Math.abs(card.y) >= 0.5)
				row.settlingDrag = true
			else
				row.settlingDrag = false
			row.dragging = false
			row.dragCardY = 0
			row.dragActiveChanged(row.sessionId, false)
		}

		Timer {
			id: holdTimer
			interval: dragArea.holdDelayMs
			repeat: false
			onTriggered: {
				if (!dragArea.pressed || dragArea.movedBeforeHold || row.renaming)
					return
				const view = row.ListView.view
				if (!view || view.count <= 1)
					return
				row.settlingDrag = false
				row.dragging = true
				dragArea.wasDragging = true
				row.dragActiveChanged(row.sessionId, true)
				const maxTop = Math.max(0, view.contentHeight - row.height)
				const relY = Math.max(0, Math.min(maxTop, row.y - view.originY))
				dragArea.dragVisualTop = view.originY + relY
				row.dragCardY = dragArea.dragVisualTop - row.y
			}
		}

		Timer {
			id: autoScrollTimer
			interval: 16
			repeat: true
			running: row.dragging && dragArea.scrollDirection !== 0
			onTriggered: {
				const view = row.ListView.view
				if (!view || !row.dragging) {
					dragArea.scrollDirection = 0
					return
				}
				const minY = view.originY
				const maxY = view.originY + Math.max(0, view.contentHeight - view.height)
				const nextY = Math.max(minY, Math.min(maxY, view.contentY + dragArea.scrollDirection * dragArea.autoScrollStep))
				if (nextY === view.contentY) {
					dragArea.scrollDirection = 0
					return
				}
				view.contentY = nextY
				dragArea.updateDrag()
			}
		}

		onPressed: mouse => {
			if (closeButton.visible) {
				const p = row.mapToItem(closeButton, mouse.x, mouse.y)
				if (p.x >= 0 && p.y >= 0 && p.x <= closeButton.width && p.y <= closeButton.height) {
					mouse.accepted = false
					return
				}
			}
			pressX = mouse.x
			pressY = mouse.y
			pressOffsetY = mouse.y
			const scenePt = dragArea.mapToItem(null, mouse.x, mouse.y)
			lastSceneX = scenePt.x
			lastSceneY = scenePt.y
			movedBeforeHold = false
			wasDragging = false
			scrollDirection = 0
			if (row.ListView.view && row.ListView.view.count > 1)
				holdTimer.start()
		}
		onPositionChanged: mouse => {
			const scenePt = dragArea.mapToItem(null, mouse.x, mouse.y)
			lastSceneX = scenePt.x
			lastSceneY = scenePt.y
			if (!row.dragging) {
				if (Math.hypot(mouse.x - pressX, mouse.y - pressY) > moveThreshold) {
					holdTimer.stop()
					movedBeforeHold = true
				}
				return
			}
			updateDrag()
		}
		onReleased: finishDrag(true)
		onCanceled: finishDrag(true)
		onEnabledChanged: if (!enabled) finishDrag(false)
		onClicked: {
			if (row.renaming || wasDragging || movedBeforeHold)
				return
			if (row.canDetach)
				Workspace.detachCurrent()
			else
				Workspace.openSession(row.sessionId)
		}
		onDoubleClicked: {
			if (row.dragging)
				return
			holdTimer.stop()
			row.startRename()
		}
		Component.onDestruction: finishDrag(false)
	}
	Connections {
		target: row.ListView.view
		function onCountChanged() {
			const view = row.ListView.view
			if (row.dragging && (!view || view.count <= 1))
				dragArea.finishDrag(false)
		}
	}
	TapHandler {
		acceptedButtons: Qt.RightButton
		enabled: !row.dragging && !row.renaming
		onTapped: point => menu.openAt(point.position)
	}

	ContextMenu {
		id: menu
		entries: row.canDetach
			? [{text: qsTr("Rename")}, {text: qsTr("Move to background")}, {text: qsTr("Close"), danger: true}]
			: [{text: qsTr("Rename")}, {text: qsTr("Close"), danger: true}]
		onTriggered: index => {
			if (index === row.renameEntry)
				row.startRename()
			else if (row.canDetach && index === 1)
				Workspace.detachCurrent()
			else
				Workspace.requestCloseSession(row.sessionId)
		}
	}
}
