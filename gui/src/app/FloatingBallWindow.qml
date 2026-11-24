pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// Floating ball mode with 3 states:
// 1) Compact 56x56 circular floating ball with status dot and running session count
// 2) Hover super-mini status card (120ms enter / 220ms exit delay)
// 3) Mini chat panel (~380x520) with session switcher, return buttons, and busy-guarded close
Item {
	id: root

	readonly property bool panelActive: ballWindow.active || hoverWindow.active || panelWindow.active
	property bool panelExpanded: false
	property string screenName: ""
	property int ballX: 0
	property int ballY: 0
	property string selectedSessionId: Workspace.currentId
	property int liveRevision: 0
	property bool confirmCloseShown: false
	readonly property int ballSize: 56
	readonly property int hoverCardWidth: 248
	readonly property int panelWidth: 380
	readonly property int panelHeight: 520

	readonly property ChatSession activeSession: {
		if (selectedSessionId !== "") {
			const picked = Workspace.session(selectedSessionId)
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
		let topTitle = ""
		let topSubtitle = ""
		for (let i = 0; i < count; ++i) {
			const row = Workspace.sessions.rowAt(i)
			const perm = row.hasPermission === true
			const err = row.hasError === true
			const act = String(row.activity ?? Activity.idle)
			if (perm)
				hasPermission = true
			if (err)
				hasError = true
			if (Activity.isWorking(act))
				working = true
			if (i === 0) {
				topTitle = String(row.displayTitle ?? "")
				topSubtitle = String(row.dirName ?? "")
			}
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
			activity: activity,
			topTitle: topTitle,
			topSubtitle: topSubtitle
		}
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
		ballX = res.x
		ballY = res.y
		screenName = res.screenName
	}

	function clampBallPosition() {
		const clamped = Workspace.floatingBallHelper.clampWindowRect(ballX, ballY, ballSize, ballSize, screenName)
		ballX = clamped.x
		ballY = clamped.y
		screenName = clamped.screenName
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
	function expandPanel() {
		hoverOpenTimer.stop()
		hoverCloseTimer.stop()
		hoverCardShown = false
		confirmCloseShown = false
		ensureSessionSelected()
		panelExpanded = true
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

	Component.onCompleted: resolvePosition()

	Connections {
		target: Workspace
		function onCurrentChanged() {
			if (Workspace.currentId !== "")
				root.selectedSessionId = Workspace.currentId
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
		}
	}

	Timer {
		id: hoverOpenTimer
		interval: Motion.quick // 120ms
		onTriggered: root.hoverCardShown = true
	}

	Timer {
		id: hoverCloseTimer
		interval: 220
		onTriggered: root.hoverCardShown = false
	}

	// 1) Compact 56x56 floating ball window.
	Window {
		id: ballWindow
		objectName: "floatingBallWindow"
		visible: Workspace.floatingMode && !root.panelExpanded
		x: root.ballX
		y: root.ballY
		width: root.ballSize
		height: root.ballSize
		color: "transparent"
		flags: Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool

		Shortcut {
			sequence: "Ctrl+Shift+F"
			enabled: ballWindow.visible
			onActivated: Commands.run(CommandId.floating)
		}

		Rectangle {
			id: ballFace
			anchors.fill: parent
			radius: width / 2
			color: ballHover.hovered ? Theme.surfaceHover : Theme.surface
			border.color: root.aggregateInfo.hasPermission ? Theme.warningBorder : (root.aggregateInfo.hasError ? Theme.danger : Theme.border)
			border.width: root.aggregateInfo.hasPermission || root.aggregateInfo.hasError ? 2 : 1
			scale: ballMouse.pressed && !ballMouse.dragged ? 0.94 : 1

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			Image {
				anchors.centerIn: parent
				anchors.verticalCenterOffset: root.aggregateInfo.count > 0 ? -4 : 0
				width: 24
				height: 24
				sourceSize: Qt.size(48, 48)
				source: "qrc:/app/ref-small.svg"
				smooth: true
				Accessible.role: Accessible.Graphic
				Accessible.name: "ref"
			}

			// Embedded running session count.
			Text {
				textFormat: Text.PlainText
				visible: root.aggregateInfo.count > 0
				anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 5 }
				text: String(root.aggregateInfo.count)
				color: Theme.textMuted
				font { pixelSize: Theme.microFontSize; bold: true }
			}

			// Breathing status dot badge.
			SessionDot {
				anchors { right: parent.right; top: parent.top; rightMargin: 6; topMargin: 6 }
				activity: root.aggregateInfo.activity
				hasError: root.aggregateInfo.hasError
				visible: root.aggregateInfo.count > 0
			}

			HoverHandler {
				id: ballHover
				cursorShape: ballMouse.dragged ? Qt.ClosedHandCursor : Qt.PointingHandCursor
				onHoveredChanged: {
					if (hovered) {
						hoverCloseTimer.stop()
						if (!ballMouse.dragged)
							hoverOpenTimer.restart()
					} else {
						hoverOpenTimer.stop()
						hoverCloseTimer.restart()
					}
				}
			}

			MouseArea {
				id: ballMouse
				anchors.fill: parent
				acceptedButtons: Qt.LeftButton | Qt.RightButton
				property real pressGlobalX: 0
				property real pressGlobalY: 0
				property int startBallX: 0
				property int startBallY: 0
				property bool dragged: false

				onPressed: mouse => {
					if (mouse.button === Qt.RightButton) {
						ballMenu.openAt(Qt.point(mouse.x, mouse.y))
						return
					}
					const gp = mapToGlobal(mouse.x, mouse.y)
					pressGlobalX = gp.x
					pressGlobalY = gp.y
					startBallX = root.ballX
					startBallY = root.ballY
					dragged = false
				}
				onPositionChanged: mouse => {
					if (!(mouse.buttons & Qt.LeftButton))
						return
					const gp = mapToGlobal(mouse.x, mouse.y)
					const dx = Math.round(gp.x - pressGlobalX)
					const dy = Math.round(gp.y - pressGlobalY)
					if (!dragged && Math.hypot(dx, dy) >= 5) {
						dragged = true
						hoverOpenTimer.stop()
						root.hoverCardShown = false
					}
					if (dragged) {
						const clamped = Workspace.floatingBallHelper.clampWindowRect(
							startBallX + dx,
							startBallY + dy,
							root.ballSize,
							root.ballSize,
							root.screenName
						)
						root.ballX = clamped.x
						root.ballY = clamped.y
						root.screenName = clamped.screenName
					}
				}
				onReleased: mouse => {
					if (mouse.button !== Qt.LeftButton)
						return
					if (dragged) {
						dragged = false
						Workspace.settings.setFloatingBallPosition(root.screenName, root.ballX, root.ballY)
						return
					}
					root.expandPanel()
				}
				onDoubleClicked: mouse => {
					if (mouse.button === Qt.LeftButton)
						Workspace.setFloatingMode(false)
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
	}

	// 2) Hover super-mini status card.
	Window {
		id: hoverWindow
		objectName: "floatingHoverCard"
		readonly property var placed: Workspace.floatingBallHelper.computePanelGeometry(
			root.ballX,
			root.ballY,
			root.ballSize,
			root.ballSize,
			root.hoverCardWidth,
			Math.max(76, hoverContent.implicitHeight + 20),
			root.screenName
		)

		visible: Workspace.floatingMode && !root.panelExpanded && root.hoverCardShown
		x: placed.x
		y: placed.y
		width: placed.width
		height: placed.height
		color: "transparent"
		flags: Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool

		Rectangle {
			anchors.fill: parent
			radius: Theme.radius
			color: Theme.surface
			border.color: root.aggregateInfo.hasPermission ? Theme.warningBorder : Theme.border

			HoverHandler {
				cursorShape: Qt.PointingHandCursor
				onHoveredChanged: {
					if (hovered) {
						hoverCloseTimer.stop()
					} else {
						hoverCloseTimer.restart()
					}
				}
			}

			TapHandler {
				onTapped: root.expandPanel()
			}

			Column {
				id: hoverContent
				x: 12
				y: 10
				width: parent.width - 24
				spacing: 4

				Row {
					width: parent.width
					spacing: 6

					SessionDot {
						anchors.verticalCenter: parent.verticalCenter
						activity: root.aggregateInfo.activity
						hasError: root.aggregateInfo.hasError
						visible: root.aggregateInfo.count > 0
					}

					Text {
						textFormat: Text.PlainText
						width: parent.width - 14
						text: root.aggregateInfo.count > 0
							? qsTr("%n running session(s)", "", root.aggregateInfo.count)
							: qsTr("No running sessions")
						color: Theme.text
						font { pixelSize: Theme.smallFontSize; bold: true }
						elide: Text.ElideRight
					}
				}

				Text {
					textFormat: Text.PlainText
					visible: root.aggregateInfo.topTitle !== ""
					width: parent.width
					text: root.aggregateInfo.topSubtitle !== ""
						? root.aggregateInfo.topTitle + " · " + root.aggregateInfo.topSubtitle
						: root.aggregateInfo.topTitle
					color: Theme.textMuted
					font.pixelSize: Theme.microFontSize
					elide: Text.ElideRight
				}

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: Workspace.connectionText !== ""
						? Workspace.connectionText
						: qsTr("Click to open · Double-click ball for full window")
					color: Theme.textMuted
					font.pixelSize: Theme.microFontSize
					elide: Text.ElideRight
				}
			}
		}
	}

	// 3) Mini chat panel (~380x520) anchored inward from the ball's screen corner.
	Window {
		id: panelWindow
		objectName: "floatingPanelWindow"
		readonly property var placed: Workspace.floatingBallHelper.computePanelGeometry(
			root.ballX,
			root.ballY,
			root.ballSize,
			root.ballSize,
			root.panelWidth,
			root.panelHeight,
			root.screenName
		)

		visible: Workspace.floatingMode && root.panelExpanded
		x: placed.x
		y: placed.y
		width: placed.width
		height: placed.height
		color: "transparent"
		flags: Qt.Window | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool

		onClosing: close => {
			if (Workspace.smoke)
				return
			if (Workspace.anySessionWorking() || Workspace.configEditor.dirty) {
				root.confirmCloseShown = true
				close.accepted = false
			}
		}

		Shortcut {
			sequence: "Ctrl+Shift+F"
			enabled: panelWindow.visible
			onActivated: Commands.run(CommandId.floating)
		}

		Rectangle {
			anchors.fill: parent
			radius: Theme.radiusLarge
			color: Theme.background
			border.color: Theme.border
			clip: true

			// Panel header: drag handle, session switcher, return & close buttons.
			Rectangle {
				id: panelHeader
				width: parent.width
				height: 44
				color: Theme.surface

				Rectangle {
					anchors.bottom: parent.bottom
					width: parent.width
					height: 1
					color: Theme.border
				}

				DragHandler {
					target: null
					dragThreshold: 4
					onActiveChanged: if (active) panelWindow.startSystemMove()
				}

				Rectangle {
					id: switcherPill
					objectName: "floatingSessionSwitcher"
					anchors { left: parent.left; leftMargin: 10; right: headerActions.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
					height: 28
					radius: 14
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
							width: Math.max(0, parent.width - 26)
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
							switcherMenu.openAt(Qt.point(0, switcherPill.height + 4))
						}
					}
				}

				Row {
					id: headerActions
					anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					spacing: 4

					IconButton {
						buttonSize: 28
						iconName: "plus"
						iconSize: 16
						enabled: Workspace.connectionState === "connected"
						tooltipText: qsTr("New session")
						Accessible.name: qsTr("New session")
						onClicked: Workspace.newSession()
					}

					IconButton {
						objectName: "floatingCollapseButton"
						buttonSize: 28
						iconName: "minus"
						iconSize: 16
						tooltipText: qsTr("Collapse to ball")
						Accessible.name: qsTr("Collapse to ball")
						onClicked: root.collapseToBall()
					}

					IconButton {
						objectName: "floatingReturnButton"
						buttonSize: 28
						iconName: "maximize"
						iconSize: 20
						tooltipText: qsTr("Return to full window")
						Accessible.name: qsTr("Return to full window")
						onClicked: Workspace.setFloatingMode(false)
					}

					IconButton {
						objectName: "floatingCloseButton"
						buttonSize: 28
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
				height: root.confirmCloseShown ? 44 : 0
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
					active: root.activeSession !== null && root.panelExpanded
					sourceComponent: SessionPane {
						session: root.activeSession
						compact: true
						keysEnabled: panelWindow.active
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
