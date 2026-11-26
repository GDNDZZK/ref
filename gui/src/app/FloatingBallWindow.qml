pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// Floating ball mode in a single always-on-top window with two views:
// 1) Compact 56x56 circular floating ball with status dot and working/total session count
// 2) Mini chat panel (~380x520) with session switcher, return buttons, and busy-guarded close
Item {
	id: root

	readonly property bool panelActive: floatingWindow.active
	readonly property bool supportsWindowPosition: !Qt.platform.pluginName.startsWith("wayland")
	property bool panelExpanded: false
	property bool positionRestored: false
	property bool syncingPosition: false
	property bool settlingGeometry: false
	property bool resizingPanel: false
	property string screenName: ""
	property int ballX: 0
	property int ballY: 0
	property int liveRevision: 0
	property bool confirmCloseShown: false
	property string draggingSessionId: ""
	readonly property bool reorderingRunning: draggingSessionId !== ""
	property real attentionScale: 1.0

	readonly property int ballSize: Workspace.settings.floatingBallSize
	readonly property int ballIconSize: Math.max(16, Math.round(ballSize * 24 / 56))
	readonly property int ballIconSourceSize: ballIconSize * 2
	readonly property int ballIconCountOffset: -Math.max(3, Math.round(ballSize * 4 / 56))
	readonly property int ballCountBottomMargin: Math.max(3, Math.round(ballSize * 5 / 56))
	readonly property int ballDotMargin: Math.max(4, Math.round(ballSize * 6 / 56))
	readonly property int dragThreshold: 4
	readonly property int moveSyncGuardMs: 150
	readonly property int panelWidth: Workspace.settings.floatingPanelWidth
	readonly property int panelHeight: Workspace.settings.floatingPanelHeight
	readonly property int panelHeaderHeight: 44
	readonly property int switcherPillHeight: 28
	readonly property int headerButtonSize: 28
	readonly property int confirmBarHeight: 44
	readonly property int headerIconReserve: 26
	readonly property int switcherMenuGap: 4
	readonly property int runningRowSpacing: 2

	readonly property ChatSession activeSession: Workspace.current
	onActiveSessionChanged: {
		if (activeSession !== null)
			confirmCloseShown = false
	}
	onBallSizeChanged: {
		if (floatingWindow.visible && !resizingPanel)
			syncWindowGeometry()
	}
	onPanelWidthChanged: {
		if (floatingWindow.visible && !resizingPanel)
			syncWindowGeometry()
	}
	onPanelHeightChanged: {
		if (floatingWindow.visible && !resizingPanel)
			syncWindowGeometry()
	}

	readonly property var aggregateInfo: {
		if (!Workspace.floatingMode) {
			return {
				count: 0,
				workingCount: 0,
				hasPermission: false,
				hasQuestion: false,
				needsAttention: false,
				hasError: false,
				working: false,
				activity: Activity.idle
			}
		}
		void Workspace.sessions.count
		void root.liveRevision
		const count = Workspace.sessions.count
		let workingCount = 0
		let hasPermission = false
		let hasQuestion = false
		let hasError = false
		for (let i = 0; i < count; ++i) {
			const row = Workspace.sessions.rowAt(i)
			if (row.hasPermission === true)
				hasPermission = true
			if (row.hasError === true)
				hasError = true
			const act = String(row.activity ?? Activity.idle)
			if (act === Activity.waitingQuestion)
				hasQuestion = true
			if (Activity.isWorking(act))
				++workingCount
		}
		const working = workingCount > 0
		const needsAttention = hasPermission || hasQuestion
		const activity = hasPermission
			? Activity.waitingPermission
			: (hasQuestion ? Activity.waitingQuestion : (working ? Activity.running : Activity.idle))
		return {
			count: count,
			workingCount: workingCount,
			hasPermission: hasPermission,
			hasQuestion: hasQuestion,
			needsAttention: needsAttention,
			hasError: hasError,
			working: working,
			activity: activity
		}
	}

	readonly property bool ballAttentionActive: Workspace.floatingMode && !root.panelExpanded && root.aggregateInfo.needsAttention

	function enforceTopMost() {
		if (floatingWindow.visible)
			Workspace.floatingBallHelper.ensureAlwaysOnTop(floatingWindow)
	}

	function syncWindowGeometry() {
		syncingPosition = true
		settlingGeometry = true
		moveSyncGuardTimer.restart()
		floatingWindow.minimumWidth = 0
		floatingWindow.maximumWidth = 16777215
		floatingWindow.minimumHeight = 0
		floatingWindow.maximumHeight = 16777215
		if (!panelExpanded) {
			floatingWindow.width = ballSize
			floatingWindow.height = ballSize
			if (supportsWindowPosition) {
				floatingWindow.x = ballX
				floatingWindow.y = ballY
			}
		} else {
			const placed = Workspace.floatingBallHelper.computePanelGeometry(
				ballX,
				ballY,
				ballSize,
				ballSize,
				panelWidth,
				panelHeight,
				screenName
			)
			if (supportsWindowPosition) {
				floatingWindow.x = placed.x
				floatingWindow.y = placed.y
			}
			floatingWindow.width = supportsWindowPosition ? placed.width : panelWidth
			floatingWindow.height = supportsWindowPosition ? placed.height : panelHeight
		}
		floatingWindow.minimumWidth = floatingWindow.width
		floatingWindow.maximumWidth = floatingWindow.width
		floatingWindow.minimumHeight = floatingWindow.height
		floatingWindow.maximumHeight = floatingWindow.height
		syncingPosition = false
		Qt.callLater(enforceTopMost)
	}

	function applyBallCoordinates(nextX, nextY, nextScreen) {
		ballX = nextX
		ballY = nextY
		screenName = nextScreen
		syncWindowGeometry()
	}

	function resolvePosition() {
		const res = Workspace.floatingBallHelper.resolveBallPosition(
			Workspace.settings.hasFloatingBallPosition,
			Workspace.settings.floatingBallScreen,
			Workspace.settings.floatingBallX,
			Workspace.settings.floatingBallY,
			ballSize,
			ballSize
		)
		applyBallCoordinates(res.x, res.y, res.screenName)
	}

	function clampBallPosition() {
		if (!supportsWindowPosition)
			return
		const clamped = Workspace.floatingBallHelper.clampWindowRect(ballX, ballY, ballSize, ballSize, screenName)
		applyBallCoordinates(clamped.x, clamped.y, clamped.screenName)
	}

	function handleWindowMoved() {
		if (!supportsWindowPosition || syncingPosition || settlingGeometry || resizingPanel || !positionRestored || !floatingWindow.visible || panelExpanded || ballDrag.active || panelDrag.active)
			return
		const nextX = floatingWindow.x
		const nextY = floatingWindow.y
		const nextScreen = floatingWindow.screen ? floatingWindow.screen.name : screenName
		if (nextX === ballX && nextY === ballY && nextScreen === screenName)
			return
		ballX = nextX
		ballY = nextY
		screenName = nextScreen
		Workspace.settings.setFloatingBallPosition(screenName, ballX, ballY)
	}

	function openSessionInPanel(sessionId) {
		confirmCloseShown = false
		if (sessionId !== "")
			Workspace.openSession(sessionId)
		panelExpanded = true
	}

	function expandPanel() {
		ballMenuWindow.visible = false
		confirmCloseShown = false
		panelExpanded = true
	}

	function collapseToBall() {
		ballMenuWindow.visible = false
		confirmCloseShown = false
		panelExpanded = false
	}

	function openBallMenu(localX, localY) {
		const menuW = Math.max(ballMenuWindow.minMenuWidth, Math.ceil(ballMenuInner.width * ballMenuWindow.uiScale))
		const menuH = Math.max(1, Math.ceil(ballMenuInner.height * ballMenuWindow.uiScale))
		ballMenuWindow.width = menuW
		ballMenuWindow.height = menuH
		if (supportsWindowPosition) {
			const placed = Workspace.floatingBallHelper.clampWindowRect(
				ballX + Math.round(localX),
				ballY + Math.round(localY),
				menuW,
				menuH,
				screenName
			)
			ballMenuWindow.x = placed.x
			ballMenuWindow.y = placed.y
		}
		ballMenuWindow.visible = true
		ballMenuContent.forceActiveFocus()
		ballMenuWindow.raise()
		ballMenuWindow.requestActivate()
		Qt.callLater(() => Workspace.floatingBallHelper.ensureAlwaysOnTop(ballMenuWindow))
	}

	function requestQuitFromPanel() {
		if (Workspace.anySessionWorking() || Workspace.configEditor.dirty) {
			confirmCloseShown = true
			return
		}
		Qt.quit()
	}

	onPanelExpandedChanged: {
		resizingPanel = false
		syncingPosition = false
		syncWindowGeometry()
		if (floatingWindow.visible) {
			floatingWindow.raise()
			if (panelExpanded)
				floatingWindow.requestActivate()
			Qt.callLater(enforceTopMost)
		}
	}

	Component.onCompleted: {
		resolvePosition()
		positionRestored = true
	}

	Timer {
		id: moveSyncGuardTimer
		interval: root.moveSyncGuardMs
		repeat: false
		onTriggered: root.settlingGeometry = false
	}

	Connections {
		target: Workspace
		function onFloatingModeChanged() {
			if (Workspace.floatingMode) {
				root.resolvePosition()
				root.positionRestored = true
				root.syncWindowGeometry()
			} else {
				ballMenuWindow.visible = false
				root.draggingSessionId = ""
				root.confirmCloseShown = false
				root.resizingPanel = false
				root.syncingPosition = false
				root.panelExpanded = false
				root.syncWindowGeometry()
			}
		}
	}

	Connections {
		target: Workspace.sessions
		function onDataChanged() { ++root.liveRevision }
		function onModelReset() { ++root.liveRevision }
		function onRowsInserted() { ++root.liveRevision }
		function onRowsRemoved() { ++root.liveRevision }
	}

	Connections {
		target: Workspace.floatingBallHelper
		function onScreenGeometryChanged() {
			root.clampBallPosition()
			root.syncWindowGeometry()
		}
	}

	// Single always-on-top window hosting both the compact ball and expanded mini panel.
	// Wayland note: compositors govern stacking and global coordinates; startSystemMove()
	// and x/y persistence gracefully degrade to compositor placement on Wayland.
	Window {
		id: floatingWindow
		objectName: "floatingBallWindow"
		// Independent of Main window, which is hidden while floating mode is active.
		transientParent: null
		visible: Workspace.floatingMode
		width: root.ballSize
		height: root.ballSize
		minimumWidth: root.ballSize
		maximumWidth: root.ballSize
		minimumHeight: root.ballSize
		maximumHeight: root.ballSize
		color: "transparent"
		flags: Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | (Qt.platform.os === "windows" ? Qt.Tool : 0)

		onVisibleChanged: {
			if (visible) {
				root.syncWindowGeometry()
				floatingWindow.raise()
				Qt.callLater(root.enforceTopMost)
			}
		}
		onActiveChanged: {
			if (!active && visible)
				Qt.callLater(root.enforceTopMost)
		}
		onVisibilityChanged: {
			if (visibility === Window.Maximized || visibility === Window.FullScreen) {
				visibility = Window.Windowed
				Qt.callLater(root.syncWindowGeometry)
			}
		}
		onXChanged: root.handleWindowMoved()
		onYChanged: root.handleWindowMoved()
		onScreenChanged: root.handleWindowMoved()
		onClosing: close => {
			if (Workspace.smoke)
				return
			if (Workspace.anySessionWorking() || Workspace.configEditor.dirty) {
				close.accepted = false
				if (!root.panelExpanded)
					root.expandPanel()
				root.confirmCloseShown = true
			}
		}

		Shortcut {
			sequence: "Ctrl+Shift+F"
			enabled: floatingWindow.visible
			onActivated: Commands.run(CommandId.floating)
		}

		// 1) Compact 56x56 circular floating ball.
		Item {
			id: ballRoot
			objectName: "floatingBallRoot"
			anchors.fill: parent
			visible: !root.panelExpanded

			SequentialAnimation {
				id: attentionNudgeAnim
				loops: Animation.Infinite
				running: root.ballAttentionActive && !ballHover.hovered && !ballMouse.pressed && !ballDrag.active
				onRunningChanged: {
					if (!running)
						root.attentionScale = 1.0
				}
				NumberAnimation { target: root; property: "attentionScale"; to: 1.06; duration: 140; easing.type: Easing.OutQuad }
				NumberAnimation { target: root; property: "attentionScale"; to: 0.98; duration: 140; easing.type: Easing.InOutQuad }
				NumberAnimation { target: root; property: "attentionScale"; to: 1.04; duration: 120; easing.type: Easing.InOutQuad }
				NumberAnimation { target: root; property: "attentionScale"; to: 1.0; duration: 160; easing.type: Easing.OutQuad }
				PauseAnimation { duration: 1400 }
			}

			Rectangle {
				id: ballFace
				anchors.fill: parent
				radius: width / 2
				color: root.aggregateInfo.needsAttention
					? (ballHover.hovered ? Theme.surfaceHover : Theme.warningSurface)
					: (ballHover.hovered ? Theme.surfaceHover : Theme.surface)
				border.color: root.aggregateInfo.needsAttention
					? Theme.warning
					: (root.aggregateInfo.hasError ? Theme.danger : (ballHover.hovered ? Theme.textMuted : Theme.border))
				border.width: root.aggregateInfo.needsAttention || root.aggregateInfo.hasError ? 2 : 1
				scale: ballMouse.pressed && !ballDrag.active ? 0.94 : root.attentionScale

				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on scale {
					enabled: !attentionNudgeAnim.running
					NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
				}

				Rectangle {
					id: attentionSonar
					objectName: "floatingBallAttentionSonar"
					anchors.fill: parent
					radius: width / 2
					color: "transparent"
					border.width: 2
					border.color: Theme.warning
					scale: 0.62
					opacity: 0
					visible: root.ballAttentionActive

					ParallelAnimation {
						loops: Animation.Infinite
						running: root.ballAttentionActive
						onRunningChanged: {
							if (!running) {
								attentionSonar.scale = 0.62
								attentionSonar.opacity = 0
							}
						}
						NumberAnimation {
							target: attentionSonar
							property: "scale"
							from: 0.62
							to: 1.0
							duration: 1400
							easing.type: Easing.OutCubic
						}
						NumberAnimation {
							target: attentionSonar
							property: "opacity"
							from: 0.65
							to: 0
							duration: 1400
							easing.type: Easing.OutCubic
						}
					}
				}

				Image {
					anchors.centerIn: parent
					anchors.verticalCenterOffset: root.aggregateInfo.count > 0 ? root.ballIconCountOffset : 0
					width: root.ballIconSize
					height: root.ballIconSize
					sourceSize: Qt.size(root.ballIconSourceSize, root.ballIconSourceSize)
					source: "qrc:/app/ref-small.svg"
					smooth: true
					Accessible.role: Accessible.Graphic
					Accessible.name: "ref"
				}

				// Embedded running session count (working/total).
				Row {
					objectName: "floatingBallCountRow"
					visible: root.aggregateInfo.count > 0
					anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: root.ballCountBottomMargin }
					spacing: 0

					Text {
						textFormat: Text.PlainText
						text: String(root.aggregateInfo.workingCount)
						color: root.aggregateInfo.needsAttention
							? Theme.warningText
							: (root.aggregateInfo.workingCount > 0 ? Theme.accentText : Theme.textMuted)
						font { pixelSize: Theme.microFontSize; bold: true }
					}

					Text {
						textFormat: Text.PlainText
						text: "/" + root.aggregateInfo.count
						color: Theme.textMuted
						font { pixelSize: Theme.microFontSize; bold: true }
					}
				}

				// Breathing status dot badge.
				SessionDot {
					anchors { right: parent.right; top: parent.top; rightMargin: root.ballDotMargin; topMargin: root.ballDotMargin }
					activity: root.aggregateInfo.activity
					hasError: root.aggregateInfo.hasError
					visible: root.aggregateInfo.count > 0
				}
			}

			HoverHandler {
				id: ballHover
				cursorShape: ballDrag.active ? Qt.ClosedHandCursor : Qt.PointingHandCursor
			}

			DragHandler {
				id: ballDrag
				target: null
				dragThreshold: root.dragThreshold
				grabPermissions: PointerHandler.CanTakeOverFromAnything

				property point dragStartGlobal: Qt.point(0, 0)
				property point dragStartWinPos: Qt.point(0, 0)

				onActiveChanged: {
					if (active) {
						moveSyncGuardTimer.stop()
						root.settlingGeometry = false
						if (!root.supportsWindowPosition) {
							floatingWindow.startSystemMove()
						} else {
							dragStartGlobal = Workspace.floatingBallHelper.cursorPos()
							dragStartWinPos = Qt.point(floatingWindow.x, floatingWindow.y)
						}
					} else {
						if (root.supportsWindowPosition) {
							root.ballX = floatingWindow.x
							root.ballY = floatingWindow.y
							if (floatingWindow.screen)
								root.screenName = floatingWindow.screen.name
							Workspace.settings.setFloatingBallPosition(root.screenName, root.ballX, root.ballY)
						}
						Qt.callLater(root.enforceTopMost)
					}
				}
				onCentroidChanged: {
					if (!active || !root.supportsWindowPosition)
						return
					const cur = Workspace.floatingBallHelper.cursorPos()
					const nextX = Math.round(dragStartWinPos.x + (cur.x - dragStartGlobal.x))
					const nextY = Math.round(dragStartWinPos.y + (cur.y - dragStartGlobal.y))
					const placed = Workspace.floatingBallHelper.clampWindowRect(nextX, nextY, root.ballSize, root.ballSize, "")
					root.syncingPosition = true
					floatingWindow.x = placed.x
					floatingWindow.y = placed.y
					root.ballX = placed.x
					root.ballY = placed.y
					root.screenName = placed.screenName
					root.syncingPosition = false
				}
			}

			MouseArea {
				id: ballMouse
				anchors.fill: parent
				acceptedButtons: Qt.LeftButton | Qt.RightButton
				onClicked: mouse => {
					if (mouse.button === Qt.RightButton)
						root.openBallMenu(mouse.x, mouse.y)
					else
						root.expandPanel()
				}
			}
		}

		// 2) Mini chat panel (~380x520) anchored inward from the ball's screen corner.
		Rectangle {
			id: panelRoot
			objectName: "floatingPanelRoot"
			anchors.fill: parent
			visible: root.panelExpanded
			radius: Theme.radiusLarge
			color: Theme.background
			border.color: Theme.border
			clip: true

			// Panel header: drag handle, session switcher, return & close buttons.
			Rectangle {
				id: panelHeader
				width: parent.width
				height: root.panelHeaderHeight
				color: Theme.surface

				Rectangle {
					anchors.bottom: parent.bottom
					width: parent.width
					height: 1
					color: Theme.border
				}

				DragHandler {
					id: panelDrag
					target: null
					dragThreshold: root.dragThreshold

					property point dragStartGlobal: Qt.point(0, 0)
					property point dragStartWinPos: Qt.point(0, 0)
					property int startBallOffsetX: 0
					property int startBallOffsetY: 0

					onActiveChanged: {
						if (active) {
							moveSyncGuardTimer.stop()
							root.settlingGeometry = false
							if (!root.supportsWindowPosition) {
								floatingWindow.startSystemMove()
							} else {
								dragStartGlobal = Workspace.floatingBallHelper.cursorPos()
								dragStartWinPos = Qt.point(floatingWindow.x, floatingWindow.y)
								startBallOffsetX = root.ballX - floatingWindow.x
								startBallOffsetY = root.ballY - floatingWindow.y
							}
						} else {
							if (root.supportsWindowPosition) {
								const targetScreen = floatingWindow.screen ? floatingWindow.screen.name : root.screenName
								const clampedBall = Workspace.floatingBallHelper.clampWindowRect(
									floatingWindow.x + startBallOffsetX,
									floatingWindow.y + startBallOffsetY,
									root.ballSize,
									root.ballSize,
									targetScreen
								)
								root.ballX = clampedBall.x
								root.ballY = clampedBall.y
								root.screenName = clampedBall.screenName
								Workspace.settings.setFloatingBallPosition(root.screenName, root.ballX, root.ballY)
							}
							Qt.callLater(root.enforceTopMost)
						}
					}
					onCentroidChanged: {
						if (!active || !root.supportsWindowPosition)
							return
						const cur = Workspace.floatingBallHelper.cursorPos()
						const nextX = Math.round(dragStartWinPos.x + (cur.x - dragStartGlobal.x))
						const nextY = Math.round(dragStartWinPos.y + (cur.y - dragStartGlobal.y))
						const placed = Workspace.floatingBallHelper.clampWindowRect(nextX, nextY, floatingWindow.width, floatingWindow.height, "")
						root.syncingPosition = true
						floatingWindow.x = placed.x
						floatingWindow.y = placed.y
						root.screenName = placed.screenName
						root.syncingPosition = false
					}
				}

				IconButton {
					id: detachButton
					objectName: "floatingDetachButton"
					anchors { left: parent.left; leftMargin: 8; verticalCenter: parent.verticalCenter }
					buttonSize: root.headerButtonSize
					iconName: "chat-list"
					iconSize: 16
					visible: root.activeSession !== null
					tooltipText: qsTr("Move to background")
					Accessible.name: qsTr("Move to background")
					onClicked: Workspace.detachCurrent()
				}

				Rectangle {
					id: switcherPill
					objectName: "floatingSessionSwitcher"
					anchors {
						left: detachButton.visible ? detachButton.right : parent.left
						leftMargin: detachButton.visible ? 6 : 10
						right: headerActions.left
						rightMargin: 8
						verticalCenter: parent.verticalCenter
					}
					height: root.switcherPillHeight
					radius: height / 2
					color: switcherHover.hovered ? Theme.surfaceHover : Theme.background
					border.color: Theme.border

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					Row {
						anchors { left: parent.left; leftMargin: 10; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
						spacing: 6

						SessionDot {
							anchors.verticalCenter: parent.verticalCenter
							activity: root.activeSession ? root.activeSession.activity : Activity.idle
							hasError: false
							visible: root.activeSession !== null
						}

						Text {
							textFormat: Text.PlainText
							width: Math.max(0, parent.width - root.headerIconReserve)
							anchors.verticalCenter: parent.verticalCenter
							text: root.activeSession
								? (root.activeSession.title !== "" ? root.activeSession.title : qsTr("Select session"))
								: (Workspace.sessions.count > 0 ? qsTr("Running sessions") : qsTr("No session"))
							color: Theme.text
							font.pixelSize: Theme.smallFontSize
							elide: Text.ElideRight
						}

						Chevron {
							anchors.verticalCenter: parent.verticalCenter
							width: 12
							height: 12
							rotation: 90
							color: Theme.textMuted
						}
					}

					HoverHandler { id: switcherHover; cursorShape: Qt.PointingHandCursor }
					TapHandler {
						onTapped: {
							const entries = []
							const count = Workspace.sessions.count
							for (let i = 0; i < count; ++i) {
								const row = Workspace.sessions.rowAt(i)
								const sid = String(row.sessionId ?? "")
								const title = String(row.displayTitle ?? sid)
								entries.push({
									text: title,
									sessionId: sid,
									checked: root.activeSession !== null && sid === root.activeSession.sessionId
								})
							if (entries.length === 0)
								return
							switcherMenu.entries = entries
							switcherMenu.openAt(Qt.point(0, switcherPill.height + root.switcherMenuGap))
						}
					}

					ContextMenu {
						id: switcherMenu
						onTriggered: index => {
							const entry = switcherMenu.entries[index]
							if (entry)
								root.openSessionInPanel(entry.sessionId)
						}
					}
				}

				Row {
					id: headerActions
					anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					spacing: 4

					IconButton {
						buttonSize: root.headerButtonSize
						iconName: "plus"
						iconSize: 16
						enabled: Workspace.connectionState === "connected"
						tooltipText: qsTr("New session")
						Accessible.name: qsTr("New session")
						onClicked: Workspace.newSession()
					}

					IconButton {
						objectName: "floatingCollapseButton"
						buttonSize: root.headerButtonSize
						iconName: "float"
						iconSize: 20
						tooltipText: qsTr("Collapse to ball")
						Accessible.name: qsTr("Collapse to ball")
						onClicked: root.collapseToBall()
					}

					IconButton {
						objectName: "floatingReturnButton"
						buttonSize: root.headerButtonSize
						iconName: "maximize"
						iconSize: 20
						tooltipText: qsTr("Return to full window")
						Accessible.name: qsTr("Return to full window")
						onClicked: Workspace.setFloatingMode(false)
					}

					IconButton {
						objectName: "floatingCloseButton"
						buttonSize: root.headerButtonSize
						iconName: "close"
						iconSize: 16
						tooltipText: qsTr("Close")
						Accessible.name: qsTr("Close")
						onClicked: root.requestQuitFromPanel()
					}
				}
			}

			// Busy-guard bar when closing while a session is working or config has unsaved edits.
			Rectangle {
				id: confirmCloseBar
				objectName: "floatingConfirmCloseBar"
				anchors { top: panelHeader.bottom; left: parent.left; right: parent.right }
				height: root.confirmCloseShown ? root.confirmBarHeight : 0
				visible: height > 0
				clip: true
				color: Theme.warningSurface

				Behavior on height { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Rectangle {
					anchors.bottom: parent.bottom
					width: parent.width
					height: 1
					color: Theme.warningBorder
				}

				Text {
					textFormat: Text.PlainText
					anchors { left: parent.left; leftMargin: 12; right: confirmActions.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
					text: qsTr("Work is still running. Quit anyway?")
					color: Theme.text
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				Row {
					id: confirmActions
					anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					spacing: 6

					DialogButton {
						small: true
						text: qsTr("Cancel")
						onClicked: root.confirmCloseShown = false
					}

					DialogButton {
						objectName: "floatingConfirmQuitButton"
						small: true
						kind: DialogButton.Danger
						text: qsTr("Quit")
						onClicked: Qt.quit()
					}
				}
			}

			Item {
				anchors { top: confirmCloseBar.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }

				Loader {
					anchors.fill: parent
					active: Workspace.floatingMode && root.activeSession !== null && root.panelExpanded
					sourceComponent: SessionPane {
						session: root.activeSession
						compact: true
						keysEnabled: floatingWindow.active
						composerShown: floatingWindow.active
					}
				}

				ListView {
					id: sessionListView
					objectName: "floatingSessionList"
					anchors.fill: parent
					clip: true
					topMargin: 8
					bottomMargin: 8
					spacing: root.runningRowSpacing
					boundsBehavior: Flickable.StopAtBounds
					interactive: !root.reorderingRunning
					visible: root.activeSession === null && Workspace.sessions.count > 0
					model: Workspace.sessions

					delegate: SessionRow {
						objectName: "floatingSessionRow"
						allowClose: false
						anyDragging: root.reorderingRunning
						pinnedCount: Workspace.sessions.pinnedCount
						onDragActiveChanged: (sessionId, active) => {
							if (active)
								root.draggingSessionId = sessionId
							else if (root.draggingSessionId === sessionId)
								root.draggingSessionId = ""
						}
						onReorderRequested: (sessionId, toIndex) => Workspace.moveRunningSession(sessionId, toIndex)
					}

					ScrollBar.vertical: ThemedScrollBar {}
				}

				Column {
					anchors.centerIn: parent
					spacing: 10
					visible: root.activeSession === null && Workspace.sessions.count === 0

					Text {
						textFormat: Text.PlainText
						anchors.horizontalCenter: parent.horizontalCenter
						text: Workspace.connectionText !== "" ? Workspace.connectionText : qsTr("No session open")
						color: Theme.textMuted
						font.pixelSize: Theme.secondaryFontSize
					}

					DialogButton {
						anchors.horizontalCenter: parent.horizontalCenter
						small: true
						text: qsTr("New session")
						enabled: Workspace.connectionState === "connected"
						onClicked: Workspace.newSession()
					}
				}
			}

			// 8-direction edge/corner drag-to-resize overlay for the expanded mini panel.
			Item {
				id: panelResizeOverlay
				objectName: "floatingPanelResizeOverlay"
				anchors.fill: parent
				z: 10
				visible: root.panelExpanded && root.supportsWindowPosition
				enabled: visible

				readonly property int topThickness: 4
				readonly property int edgeThickness: 6
				readonly property int cornerSize: 10

				component ResizeHandle: MouseArea {
					id: handle

					required property int edges

					property point startCursor: Qt.point(0, 0)
					property rect startRect: Qt.rect(0, 0, 0, 0)

					function finishResize() {
						if (!root.resizingPanel)
							return
						floatingWindow.minimumWidth = floatingWindow.width
						floatingWindow.maximumWidth = floatingWindow.width
						floatingWindow.minimumHeight = floatingWindow.height
						floatingWindow.maximumHeight = floatingWindow.height
						root.syncingPosition = false
						Workspace.settings.setFloatingPanelSize(floatingWindow.width, floatingWindow.height)
						root.resizingPanel = false
						Qt.callLater(root.enforceTopMost)
					}

					acceptedButtons: Qt.LeftButton
					preventStealing: true

					onPressed: mouse => {
						if (mouse.button !== Qt.LeftButton || !root.panelExpanded || !root.supportsWindowPosition)
							return
						mouse.accepted = true
						moveSyncGuardTimer.stop()
						root.settlingGeometry = false
						root.resizingPanel = true
						root.syncingPosition = true
						startCursor = Workspace.floatingBallHelper.cursorPos()
						startRect = Qt.rect(floatingWindow.x, floatingWindow.y, floatingWindow.width, floatingWindow.height)
						floatingWindow.minimumWidth = 0
						floatingWindow.maximumWidth = 16777215
						floatingWindow.minimumHeight = 0
						floatingWindow.maximumHeight = 16777215
					}
					onPositionChanged: {
						if (!pressed || !root.resizingPanel)
							return
						const cur = Workspace.floatingBallHelper.cursorPos()
						const dx = cur.x - startCursor.x
						const dy = cur.y - startCursor.y
						const minW = Workspace.settings.minFloatingPanelWidth
						const maxW = Workspace.settings.maxFloatingPanelWidth
						const minH = Workspace.settings.minFloatingPanelHeight
						const maxH = Workspace.settings.maxFloatingPanelHeight
						const startRight = startRect.x + startRect.width
						const startBottom = startRect.y + startRect.height

						let nextX = startRect.x
						let nextW = startRect.width
						if ((edges & Qt.LeftEdge) !== 0) {
							nextW = Math.max(minW, Math.min(maxW, Math.round(startRect.width - dx)))
							nextX = startRight - nextW
						} else if ((edges & Qt.RightEdge) !== 0) {
							nextW = Math.max(minW, Math.min(maxW, Math.round(startRect.width + dx)))
						}

						let nextY = startRect.y
						let nextH = startRect.height
						if ((edges & Qt.TopEdge) !== 0) {
							nextH = Math.max(minH, Math.min(maxH, Math.round(startRect.height - dy)))
							nextY = startBottom - nextH
						} else if ((edges & Qt.BottomEdge) !== 0) {
							nextH = Math.max(minH, Math.min(maxH, Math.round(startRect.height + dy)))
						}

						const placed = Workspace.floatingBallHelper.clampWindowRect(nextX, nextY, nextW, nextH, root.screenName)
						const effMinW = Math.min(minW, placed.width)
						const effMinH = Math.min(minH, placed.height)

						let finalX = placed.x
						let finalW = placed.width
						if ((edges & Qt.LeftEdge) !== 0) {
							finalW = Math.max(effMinW, Math.min(placed.width, startRight - placed.x))
							finalX = Math.max(placed.x, startRight - finalW)
						} else if ((edges & Qt.RightEdge) !== 0) {
							const screenRight = placed.x + placed.width
							finalW = Math.max(effMinW, Math.min(nextW, screenRight - nextX))
							finalX = Math.min(nextX, screenRight - finalW)
						}

						let finalY = placed.y
						let finalH = placed.height
						if ((edges & Qt.TopEdge) !== 0) {
							finalH = Math.max(effMinH, Math.min(placed.height, startBottom - placed.y))
							finalY = Math.max(placed.y, startBottom - finalH)
						} else if ((edges & Qt.BottomEdge) !== 0) {
							const screenBottom = placed.y + placed.height
							finalH = Math.max(effMinH, Math.min(nextH, screenBottom - nextY))
							finalY = Math.min(nextY, screenBottom - finalH)
						}

						floatingWindow.x = finalX
						floatingWindow.y = finalY
						floatingWindow.width = finalW
						floatingWindow.height = finalH
						root.screenName = placed.screenName
					}
					onReleased: finishResize()
					onCanceled: finishResize()
				}

				ResizeHandle {
					edges: Qt.TopEdge
					x: panelResizeOverlay.cornerSize
					y: 0
					width: Math.max(0, parent.width - 2 * panelResizeOverlay.cornerSize)
					height: panelResizeOverlay.topThickness
					cursorShape: Qt.SizeVerCursor
				}
				ResizeHandle {
					edges: Qt.BottomEdge
					x: panelResizeOverlay.cornerSize
					y: parent.height - panelResizeOverlay.edgeThickness
					width: Math.max(0, parent.width - 2 * panelResizeOverlay.cornerSize)
					height: panelResizeOverlay.edgeThickness
					cursorShape: Qt.SizeVerCursor
				}
				ResizeHandle {
					edges: Qt.LeftEdge
					x: 0
					y: panelResizeOverlay.cornerSize
					width: panelResizeOverlay.edgeThickness
					height: Math.max(0, parent.height - 2 * panelResizeOverlay.cornerSize)
					cursorShape: Qt.SizeHorCursor
				}
				ResizeHandle {
					edges: Qt.RightEdge
					x: parent.width - panelResizeOverlay.edgeThickness
					y: panelResizeOverlay.cornerSize
					width: panelResizeOverlay.edgeThickness
					height: Math.max(0, parent.height - 2 * panelResizeOverlay.cornerSize)
					cursorShape: Qt.SizeHorCursor
				}
				ResizeHandle {
					edges: Qt.TopEdge | Qt.LeftEdge
					x: 0
					y: 0
					width: panelResizeOverlay.cornerSize
					height: panelResizeOverlay.cornerSize
					cursorShape: Qt.SizeFDiagCursor
				}
				ResizeHandle {
					edges: Qt.TopEdge | Qt.RightEdge
					x: parent.width - panelResizeOverlay.cornerSize
					y: 0
					width: panelResizeOverlay.cornerSize
					height: panelResizeOverlay.cornerSize
					cursorShape: Qt.SizeBDiagCursor
				}
				ResizeHandle {
					edges: Qt.BottomEdge | Qt.LeftEdge
					x: 0
					y: parent.height - panelResizeOverlay.cornerSize
					width: panelResizeOverlay.cornerSize
					height: panelResizeOverlay.cornerSize
					cursorShape: Qt.SizeBDiagCursor
				}
				ResizeHandle {
					edges: Qt.BottomEdge | Qt.RightEdge
					x: parent.width - panelResizeOverlay.cornerSize
					y: parent.height - panelResizeOverlay.cornerSize
					width: panelResizeOverlay.cornerSize
					height: panelResizeOverlay.cornerSize
					cursorShape: Qt.SizeFDiagCursor
				}
			}
		}
	}

	// Top-level right-click context menu for the 56x56 floating ball so it is never clipped to 56x56.
	Window {
		id: ballMenuWindow
		objectName: "floatingBallContextMenu"
		transientParent: floatingWindow
		visible: false
		width: 160
		height: 108
		color: "transparent"
		flags: Qt.Popup | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint

		readonly property int textInset: 14
		readonly property int menuPadding: 4
		readonly property int minMenuWidth: 156
		readonly property real uiScale: UiScale.factor
		readonly property var entries: [
			{text: qsTr("Open mini panel"), action: "expand"},
			{text: qsTr("Return to full window"), action: "restore"},
			{text: qsTr("New session"), action: "new"}
		]

		onActiveChanged: {
			if (!active)
				visible = false
		}

		Item {
			id: ballMenuContent
			anchors.fill: parent
			focus: true
			Keys.onEscapePressed: event => {
				ballMenuWindow.visible = false
				event.accepted = true
			}

			Item {
				id: ballMenuInner
				width: Math.max(ballMenuWindow.minMenuWidth, ballMenuList.implicitWidth + 2 * ballMenuWindow.menuPadding)
				height: ballMenuList.implicitHeight + 2 * ballMenuWindow.menuPadding
				transformOrigin: Item.TopLeft
				scale: ballMenuWindow.uiScale

				Rectangle {
					anchors.fill: parent
					radius: Theme.radius
					color: Theme.surface
					border.color: Theme.border
				}

				Column {
					id: ballMenuList
					x: ballMenuWindow.menuPadding
					y: ballMenuWindow.menuPadding
					spacing: 2

					Repeater {
						model: ballMenuWindow.entries

						Item {
							id: entry

							required property var modelData
							required property int index

							width: Math.max(150, label.implicitWidth + 2 * ballMenuWindow.textInset)
							height: 32

							Rectangle {
								anchors.fill: parent
								radius: Theme.radiusSmall + 2
								color: entryMouse.containsMouse ? Theme.surfaceHover : "transparent"

								Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
							}

							Text {
								id: label
								textFormat: Text.PlainText
								anchors { left: parent.left; leftMargin: ballMenuWindow.textInset; verticalCenter: parent.verticalCenter }
								text: entry.modelData.text
								color: entry.modelData.danger === true ? Theme.dangerText : Theme.text
								font.pixelSize: Theme.secondaryFontSize
							}

							MouseArea {
								id: entryMouse
								anchors.fill: parent
								hoverEnabled: true
								cursorShape: Qt.PointingHandCursor
								onClicked: {
									ballMenuWindow.visible = false
									const action = entry.modelData.action
									if (action === "expand") {
										root.expandPanel()
									} else if (action === "restore") {
										Workspace.setFloatingMode(false)
									} else if (action === "new") {
										Workspace.newSession()
										root.expandPanel()
									}
								}
							}
						}
					}
				}
			}
		}
	}
}
