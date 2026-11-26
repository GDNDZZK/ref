pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Floating ball mode in a single always-on-top window with two views:
// 1) Compact 56x56 circular floating ball with status dot and running session count
// 2) Mini chat panel (~380x520) with session switcher, return buttons, and busy-guarded close
Item {
	id: root

	readonly property bool panelActive: floatingWindow.active
	readonly property bool supportsWindowPosition: !Qt.platform.pluginName.startsWith("wayland")
	property bool panelExpanded: false
	property bool positionRestored: false
	property bool syncingPosition: false
	property bool settlingGeometry: false
	property string screenName: ""
	property int ballX: 0
	property int ballY: 0
	property string selectedSessionId: ""
	property int liveRevision: 0
	property bool confirmCloseShown: false

	readonly property int ballSize: 56
	readonly property int ballIconSize: 24
	readonly property int ballIconSourceSize: 48
	readonly property int ballIconCountOffset: -4
	readonly property int ballCountBottomMargin: 5
	readonly property int ballDotMargin: 6
	readonly property int dragThreshold: 4
	readonly property int moveSyncGuardMs: 150
	readonly property int panelWidth: 380
	readonly property int panelHeight: 520
	readonly property int panelHeaderHeight: 44
	readonly property int switcherPillHeight: 28
	readonly property int headerButtonSize: 28
	readonly property int confirmBarHeight: 44
	readonly property int headerIconReserve: 26
	readonly property int switcherMenuGap: 4

	readonly property ChatSession activeSession: {
		if (selectedSessionId !== "") {
			const picked = Workspace.findSession(selectedSessionId)
			if (picked !== null)
				return picked
		}
		return Workspace.current
	}

	readonly property var aggregateInfo: {
		void Workspace.sessions.count
		void root.liveRevision
		const count = Workspace.sessions.count
		let hasPermission = false
		let hasError = false
		let working = false
		for (let i = 0; i < count; ++i) {
			const row = Workspace.sessions.rowAt(i)
			if (row.hasPermission === true)
				hasPermission = true
			if (row.hasError === true)
				hasError = true
			const act = String(row.activity ?? Activity.idle)
			if (Activity.isWorking(act))
				working = true
		}
		let activity = Activity.idle
		if (hasPermission)
			activity = Activity.waitingPermission
		else if (working)
			activity = Activity.running
		return {
			count: count,
			hasPermission: hasPermission,
			hasError: hasError,
			working: working,
			activity: activity
		}
	}

	function syncWindowGeometry() {
		syncingPosition = true
		settlingGeometry = true
		moveSyncGuardTimer.restart()
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
		syncingPosition = false
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
		if (!supportsWindowPosition || syncingPosition || settlingGeometry || !positionRestored || !floatingWindow.visible || panelExpanded)
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

	function ensureSessionSelected() {
		if (Workspace.currentId !== "") {
			selectedSessionId = Workspace.currentId
			return
		}
		if (Workspace.sessions.count > 0) {
			const row = Workspace.sessions.rowAt(0)
			const firstId = String(row.sessionId ?? "")
			if (firstId !== "") {
				selectedSessionId = firstId
				Workspace.openSession(firstId)
			}
		}
	}

	function openSessionInPanel(sessionId) {
		confirmCloseShown = false
		if (sessionId !== "") {
			selectedSessionId = sessionId
			Workspace.openSession(sessionId)
		} else {
			ensureSessionSelected()
		}
		panelExpanded = true
	}

	function expandPanel() {
		openSessionInPanel("")
	}

	function collapseToBall() {
		confirmCloseShown = false
		panelExpanded = false
	}

	function requestQuitFromPanel() {
		if (Workspace.anySessionWorking() || Workspace.configEditor.dirty) {
			confirmCloseShown = true
			return
		}
		Qt.quit()
	}

	onPanelExpandedChanged: {
		syncWindowGeometry()
		if (floatingWindow.visible) {
			floatingWindow.raise()
			if (panelExpanded)
				floatingWindow.requestActivate()
		}
	}

	Component.onCompleted: {
		resolvePosition()
		positionRestored = true
		if (Workspace.currentId !== "")
			selectedSessionId = Workspace.currentId
	}

	Timer {
		id: moveSyncGuardTimer
		interval: root.moveSyncGuardMs
		repeat: false
		onTriggered: root.settlingGeometry = false
	}

	Connections {
		target: Workspace
		function onCurrentChanged() {
			root.selectedSessionId = Workspace.currentId
		}
		function onSessionDropped(session) {
			if (session && session.sessionId === root.selectedSessionId)
				root.selectedSessionId = Workspace.currentId
		}
		function onFloatingModeChanged() {
			if (Workspace.floatingMode) {
				root.resolvePosition()
				root.positionRestored = true
				if (Workspace.currentId !== "")
					root.selectedSessionId = Workspace.currentId
				root.syncWindowGeometry()
			} else {
				root.confirmCloseShown = false
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
		color: "transparent"
		flags: Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | (Qt.platform.os === "windows" ? Qt.Tool : 0)

		onVisibleChanged: {
			if (visible) {
				root.syncWindowGeometry()
				floatingWindow.raise()
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

			Rectangle {
				id: ballFace
				anchors.fill: parent
				radius: width / 2
				color: ballHover.hovered ? Theme.surfaceHover : Theme.surface
				border.color: root.aggregateInfo.hasPermission
					? Theme.warningBorder
					: (root.aggregateInfo.hasError ? Theme.danger : (ballHover.hovered ? Theme.textMuted : Theme.border))
				border.width: root.aggregateInfo.hasPermission || root.aggregateInfo.hasError ? 2 : 1
				scale: ballMouse.pressed && !ballDrag.active ? 0.94 : 1

				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

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

				// Embedded running session count.
				Text {
					textFormat: Text.PlainText
					visible: root.aggregateInfo.count > 0
					anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: root.ballCountBottomMargin }
					text: String(root.aggregateInfo.count)
					color: Theme.textMuted
					font { pixelSize: Theme.microFontSize; bold: true }
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
				onActiveChanged: {
					if (active) {
						moveSyncGuardTimer.stop()
						root.settlingGeometry = false
						floatingWindow.startSystemMove()
					}
				}
			}

			MouseArea {
				id: ballMouse
				anchors.fill: parent
				acceptedButtons: Qt.LeftButton | Qt.RightButton
				onClicked: mouse => {
					if (mouse.button === Qt.RightButton)
						ballMenu.openAt(Qt.point(mouse.x, mouse.y))
					else
						root.expandPanel()
				}
			}

			ContextMenu {
				id: ballMenu
				entries: [
					{text: qsTr("Open mini panel"), action: "expand"},
					{text: qsTr("Return to full window"), action: "restore"},
					{text: qsTr("New session"), action: "new"}
				]
				onTriggered: index => {
					const entry = ballMenu.entries[index]
					if (!entry)
						return
					if (entry.action === "expand") {
						root.expandPanel()
					} else if (entry.action === "restore") {
						Workspace.setFloatingMode(false)
					} else if (entry.action === "new") {
						Workspace.newSession()
						root.expandPanel()
					}
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
					target: null
					dragThreshold: root.dragThreshold
					onActiveChanged: if (active) floatingWindow.startSystemMove()
				}

				Rectangle {
					id: switcherPill
					objectName: "floatingSessionSwitcher"
					anchors { left: parent.left; leftMargin: 10; right: headerActions.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
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
							text: root.activeSession && root.activeSession.title !== ""
								? root.activeSession.title
								: (Workspace.sessions.count > 0 ? qsTr("Select session") : qsTr("No session"))
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
								entries.push({text: title, sessionId: sid})
							}
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
						iconName: "minus"
						iconSize: 16
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
						composerShown: true
					}
				}

				Column {
					anchors.centerIn: parent
					spacing: 10
					visible: root.activeSession === null

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
		}
	}
}
