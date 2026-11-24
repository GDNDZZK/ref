import QtQuick
import RefGui
// One session in split view: a header with its state and the session pane below. The pane is
// the same item while the tile is zoomed, so scroll position and streaming survive.
Item {
	id: tile

	required property ChatSession session
	required property string sessionId
	required property bool leaving
	required property int place
	required property bool anyZoomed // whether any tile is zoomed

	property bool sidebarAnimating: false
	property real stableWidth: width
	// Where the grid wants the tile, and whether it is the zoomed one.
	property rect slot: Qt.rect(0, 0, 0, 0)
	// The slot as it was when the tile started leaving: the tile fades in place while the
	// others reflow, and once revived it follows the live `slot` again.
	property rect frozenSlot: Qt.rect(0, 0, 0, 0)
	readonly property rect placedSlot: leaving ? frozenSlot : slot
	property rect zoomRect: Qt.rect(0, 0, 0, 0)
	property bool zoomed: Workspace.zoomedId === sessionId
	readonly property bool expanded: zoomed
	readonly property bool fading: leaving && Workspace.splitView
	readonly property bool othersZoomed: anyZoomed && !zoomed
	// Whether the tile moves between its slot and the whole area (slowly) or just reflows.
	property bool unzooming: false
	readonly property bool travelling: expanded || unzooming
	readonly property int travelDuration: travelling || leaving ? Motion.slow : Motion.standard
	readonly property bool focused: Workspace.focusedTileId === sessionId
	readonly property var facts: live.data
	readonly property bool hasPermission: facts.hasPermission === true
	readonly property bool failed: Activity.isFailed(facts.activity, facts.hasError === true)
	readonly property string rootId: facts.rootId ?? ""
	readonly property string rootName: facts.rootName ?? ""
	readonly property bool hasRoot: rootId !== ""
	readonly property color rootColor: hasRoot ? Theme.hashColor(rootId) : "transparent"
	readonly property bool rootTinted: hasRoot && !focused && !expanded
	function focusPane() {
		pane.focusInput()
	}
	function finishResizeLayout() {
		if (!resizingLayout || resizeSettleTimer.running || widthAnim.running || sidebarAnimating)
			return
		settledLayoutWidth = paneTargetWidth
		resizingLayout = false
	}
	function canStartEnlargeResize() {
		return expanded && arrived && !sidebarAnimating && paneTargetWidth > pane.width + 1
	}

	readonly property int targetBorderWidth: expanded ? 0 : 1
	readonly property real targetWidth: expanded ? zoomRect.width : placedSlot.width
	readonly property real paneTargetWidth: Math.max(0, targetWidth - 2 * targetBorderWidth)
	property real paneWidthFloor: paneTargetWidth
	property bool arrived: false
	property bool resizingLayout: false
	property real settledLayoutWidth: 0
	readonly property int resizeSettleMs: 130
	readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)
	x: expanded ? zoomRect.x : placedSlot.x
	y: expanded ? zoomRect.y : placedSlot.y
	width: targetWidth
	height: expanded ? zoomRect.height : placedSlot.height
	z: expanded ? 1 : fading ? -1 : 0
	opacity: othersZoomed || fading ? 0 : 1
	scale: othersZoomed || fading ? Motion.tileExitScale : 1
	visible: opacity > 0
	enabled: !leaving

	Behavior on x { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on y { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on width {
		NumberAnimation {
			id: widthAnim
			duration: tile.travelDuration
			easing.type: Motion.ease
			onRunningChanged: if (!running) tile.finishResizeLayout()
		}
	}
	Behavior on height { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	onPaneTargetWidthChanged: {
		paneWidthFloor = pane.width > 0 ? Math.min(pane.width, paneTargetWidth) : paneTargetWidth
		if (canStartEnlargeResize()) {
			resizingLayout = true
			resizeSettleTimer.restart()
			return
		}
		if (!arrived || travelling) {
			settledLayoutWidth = paneTargetWidth
			resizingLayout = false
			resizeSettleTimer.stop()
			return
		}
		if (Math.abs(paneTargetWidth - settledLayoutWidth) > 1) {
			resizingLayout = true
			resizeSettleTimer.restart()
		}
	}
	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: tile.fading ? Motion.exitEase : Motion.ease } }
	Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: tile.fading ? Motion.exitEase : Motion.ease } }

	// A tile that leaves stays where it was while it fades; the others reflow into its slot.
	onLeavingChanged: {
		if (leaving)
			frozenSlot = slot
	}

	onExpandedChanged: {
		if (expanded) {
			unzooming = true
			settle.stop()
			if (canStartEnlargeResize()) {
				resizingLayout = true
				resizeSettleTimer.restart()
			}
		} else {
			settle.restart()
		}
	}

	Timer {
		id: settle
		interval: Motion.slow
		onTriggered: {
			tile.unzooming = false
			if (!tile.resizingLayout)
				tile.settledLayoutWidth = tile.paneTargetWidth
		}
	}

	Timer {
		id: resizeSettleTimer
		interval: tile.resizeSettleMs
		onTriggered: tile.finishResizeLayout()
	}

	onSidebarAnimatingChanged: {
		if (sidebarAnimating) {
			if (arrived && !travelling) {
				resizingLayout = true
				resizeSettleTimer.restart()
			}
		} else if (resizingLayout) {
			resizeSettleTimer.restart()
		} else {
			settledLayoutWidth = paneTargetWidth
		}
	}

	// A tile arrives by fading and growing into its slot.
	Component.onCompleted: {
		settledLayoutWidth = paneTargetWidth
		if (expanded)
			unzooming = true
		arrival.start()
	}
	ParallelAnimation {
		id: arrival
		NumberAnimation { target: frame; property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: frame; property: "scale"; from: Motion.tileEnterScale; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		onFinished: {
			tile.arrived = true
			if (!tile.resizingLayout)
				tile.settledLayoutWidth = tile.paneTargetWidth
		}
	}

	LiveInfo {
		id: live
		sessionId: tile.sessionId
	}

	Ticker {
		id: ticker
		interval: Format.msPerSecond
		running: tile.facts.busySince > 0 && tile.visible
	}

	Rectangle {
		id: frame
		anchors.fill: parent
		radius: tile.expanded ? 0 : Theme.radius
		color: Theme.background
		border.width: tile.targetBorderWidth
		border.color: tile.fading ? Theme.background : (tile.focused || tile.expanded ? Theme.accentText : (tile.hasRoot ? tile.rootColor : Theme.border))
		layer.enabled: !tile.travelling && (tile.fading || tile.othersZoomed)
		layer.smooth: true
		layer.textureSize: Qt.size(Math.max(1, Math.ceil(width * tile.pixelScale)), Math.max(1, Math.ceil(height * tile.pixelScale)))

		Behavior on radius { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		Rectangle {
			id: header
			x: frame.border.width
			y: frame.border.width
			width: parent.width - 2 * frame.border.width
			height: tile.expanded ? 0 : Theme.tileHeaderHeight
			topLeftRadius: Math.max(0, frame.radius - frame.border.width)
			topRightRadius: topLeftRadius
			color: tile.hasPermission ? Theme.warningSurface : Theme.surface
			opacity: tile.expanded ? 0 : 1
			clip: true
			Behavior on height { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
			Behavior on opacity { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
			Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Rectangle {
				anchors.bottom: parent.bottom
				width: parent.width
				height: 1
				color: tile.hasPermission ? Theme.warningBorder : Theme.border
			}

			SessionDot {
				x: 12
				anchors.verticalCenter: parent.verticalCenter
				activity: tile.facts.activity ?? Activity.idle
				hasError: tile.facts.hasError ?? false
				tint: tile.rootTinted ? tile.rootColor : "transparent"
			}

			Item {
				anchors { left: parent.left; leftMargin: 30; right: status.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
				height: parent.height

				TextMetrics {
					id: titleMetrics
					font: titleText.font
					text: titleText.text
				}

				Text {
					id: titleText
					textFormat: Text.PlainText
					anchors { left: parent.left; verticalCenter: parent.verticalCenter }
					width: rootChip.visible ? Math.min(Math.ceil(titleMetrics.width), Math.max(0, parent.width - rootChip.width - 8)) : parent.width
					text: tile.facts.displayTitle ?? ""
					color: Theme.text
					font.pixelSize: Theme.compactFontSize
					elide: Text.ElideRight
				}

				Rectangle {
					id: rootChip
					readonly property int maxChipWidth: Math.min(160, Math.floor(parent.width * 0.45))
					visible: tile.hasRoot && parent.width > 80
					anchors { left: titleText.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					width: Math.min(rootLabel.implicitWidth + 12, maxChipWidth)
					height: 20
					radius: 4
					color: Qt.alpha(tile.rootColor, 0.1)
					border.color: Qt.alpha(tile.rootColor, 0.35)

					Text {
						id: rootLabel
						textFormat: Text.PlainText
						anchors { left: parent.left; right: parent.right; leftMargin: 6; rightMargin: 6; verticalCenter: parent.verticalCenter }
						text: "\u27f2 " + tile.rootName
						color: tile.rootColor
						font.pixelSize: Theme.microFontSize
						elide: Text.ElideRight
					}

					HoverHandler { id: rootHover }
					PlainToolTip {
						visible: rootHover.hovered && rootLabel.truncated
						text: tile.rootName
					}
				}
			}
			Row {
				id: status
				anchors { right: zoomButton.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
				spacing: 8

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: tile.facts.activity === Activity.waitingQuestion ? "question" : "lock"
					size: 16
					color: Theme.warningText
					visible: tile.hasPermission
				}
				AlertBadge {
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: 14
					implicitHeight: 14
					visible: tile.failed
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					visible: tile.facts.todoTotal > 0
					text: (tile.facts.todoAllDone ? "✓" : "") + tile.facts.todoDone + "/" + tile.facts.todoTotal
					color: tile.facts.todoAllDone ? Theme.successText : Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					visible: tile.facts.busySince > 0
					text: Format.elapsed(ticker.now - tile.facts.busySince)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
			}

			IconButton {
				id: zoomButton
				anchors { right: parent.right; rightMargin: 4; verticalCenter: parent.verticalCenter }
				buttonSize: 28
				iconName: "maximize"
				Accessible.name: qsTr("Zoom in")
				onClicked: Workspace.zoomTile(tile.sessionId)
			}

			TapHandler {
				acceptedButtons: Qt.LeftButton
				onTapped: eventPoint => {
					if (zoomButton.contains(header.mapToItem(zoomButton, eventPoint.position)))
						return
					if (tile.focused) {
						Workspace.focusTile("")
					} else {
						Workspace.focusTile(tile.sessionId)
						pane.focusInput()
					}
				}
				onDoubleTapped: eventPoint => {
					if (!zoomButton.contains(header.mapToItem(zoomButton, eventPoint.position)))
						Workspace.zoomTile(tile.sessionId)
				}
			}
			TapHandler {
				acceptedButtons: Qt.RightButton
				onTapped: point => {
					const items = []
					const parentId = tile.facts.parentId ?? ""
					if (parentId !== "") {
						const creatorName = tile.facts.creatorName || parentId
						items.push({text: qsTr("Filter tiles by %1").arg(creatorName), action: "filterCreator", targetId: parentId})
					}
					items.push({text: qsTr("Filter tiles spawned by %1").arg(tile.facts.displayTitle || tile.sessionId), action: "filterSpawned", targetId: tile.sessionId})
					items.push({text: qsTr("Filter spawned history"), action: "filterHistory", targetId: tile.sessionId})
					if (Workspace.splitFilter !== "all")
						items.push({text: qsTr("Show all tiles"), action: "showAll", targetId: ""})
					headerMenu.entries = items
					headerMenu.openAt(point.position)
				}
			}
			ContextMenu {
				id: headerMenu
				onTriggered: index => {
					const entry = headerMenu.entries[index]
					if (!entry)
						return
					if (entry.action === "filterCreator" || entry.action === "filterSpawned")
						Workspace.setSplitFilter(entry.targetId)
					else if (entry.action === "filterHistory")
						Workspace.filterHistoryByCreator(entry.targetId)
					else if (entry.action === "showAll")
						Workspace.setSplitFilter("all")
				}
			}
		}
		SessionPane {
			id: pane
			anchors {
				top: header.bottom
				left: parent.left
				right: parent.right
				bottom: parent.bottom
				leftMargin: frame.border.width
				rightMargin: frame.border.width
				bottomMargin: frame.border.width
			}
			session: tile.session
			compact: !tile.expanded
			layoutCompact: widthAnim.running ? true : !tile.expanded
			layoutWidth: {
				const inset = anchors.leftMargin + anchors.rightMargin
				if (tile.resizingLayout)
					return tile.settledLayoutWidth
				if (tile.sidebarAnimating)
					return Math.max(0, tile.stableWidth - inset)
				if (widthAnim.running)
					return tile.paneWidthFloor
				return width
			}
			keysEnabled: tile.focused || tile.expanded
			composerShown: tile.focused || tile.expanded
		}
		// A click anywhere in an unfocused pane focuses the tile, and its input box. The overlay
		// sits in front so that it sees the press first while letting selectable text and buttons
		// handle the click as well.
		Item {
			anchors.fill: pane

			TapHandler {
				enabled: !tile.focused
				acceptedButtons: Qt.LeftButton
				gesturePolicy: TapHandler.DragThreshold
				onTapped: eventPoint => {
					if (pane.isJumpButtonAt(eventPoint.scenePosition))
						return
					Workspace.focusTile(tile.sessionId)
					pane.focusInput()
				}
			}
		}
	}

	Connections {
		target: Workspace
		function onFocusedTileChanged() {
			// Once the input box is there to take the keyboard.
			Qt.callLater(() => {
				if (tile.focused)
					pane.focusInput()
			})
		}
	}
}
