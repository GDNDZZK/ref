pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// The sessions of the window: the to-do list of the session in front, those running, the past ones with a
// search field, and the MCP status. The content has a fixed width; Main animates the width of the sidebar itself, so
// the content slides out to the left, clipped.
Rectangle {
	id: sidebar

	property int preferredWidth: Theme.sidebarWidth
	property real maxAllowed: Theme.sidebarMaxWidth
	readonly property int contentWidth: Math.max(Theme.sidebarMinWidth, Math.min(maxAllowed, preferredWidth))
	readonly property alias resizing: resizeDrag.active
	readonly property bool resizingSections: todoResizeDrag.active || runningResizeDrag.active
	property string draggingSessionId: ""
	readonly property bool reorderingRunning: draggingSessionId !== ""
	readonly property int newRowHeight: 44
	readonly property int searchHeight: 32
	readonly property int historyGap: 8 // between the search field and the history rows
	readonly property int runningRowSpacing: 2
	readonly property real minRunningHeight: Theme.sidebarMinRunningRows * (Theme.sidebarRowHeight + runningRowSpacing)
	readonly property real minHistoryHeight: Theme.sidebarMinHistoryRows * Theme.sidebarHistoryRowHeight
	readonly property real chromeHeight: Theme.sidebarHeaderHeight + newRowHeight + 2 * Theme.sidebarSectionHeight + Theme.sidebarFooterHeight
	readonly property real bodySpace: Math.max(0, height - chromeHeight)
	readonly property real historyOverhead: searchHeight + historyGap
	readonly property bool todoExpanded: Workspace.settings.sidebarTodoExpanded
	readonly property bool runningExpanded: Workspace.settings.sidebarRunningExpanded
	readonly property bool historyExpanded: Workspace.settings.sidebarHistoryExpanded
	readonly property real reservedRunning: runningExpanded ? minRunningHeight : 0
	readonly property real reservedHistory: historyExpanded ? (historyOverhead + minHistoryHeight) : 0
	property bool ready: false

	// The to-do list of the session in front: the current one, or the focused tile in split view. One value, {items,
	// done, total}, and nothing else is derived from it by a binding that could be read before it has caught up. The
	// section keeps what it showed while it closes, and changes only when the list does.
	readonly property var front: Workspace.frontSession
	readonly property var todo: front ? front.todo : ({ items: [], done: 0, total: 0 })
	property var shownTodo: ({ items: [], done: 0, total: 0 })
	readonly property real minCustomTodoHeight: Theme.sidebarSectionHeight + todoList.rowHeight
	readonly property real minCustomRunningHeight: Theme.sidebarRowHeight
	readonly property real minCustomHistoryHeight: historyOverhead + Theme.sidebarHistoryRowHeight
	// Up to a third of the column, and never so much that expanded lists below lose their minimum.
	readonly property real todoCap: Math.max(0, Math.min(height / 3, bodySpace - reservedRunning - reservedHistory))
	readonly property bool todoFits: todo.items.length > 0 && todoCap >= Theme.sidebarSectionHeight + todoList.rowHeight
	readonly property real minTodoBodyHeight: todoList.rowHeight + 2 * todoList.verticalPadding
	readonly property real customTodoMax: Math.max(minCustomTodoHeight, bodySpace - (runningExpanded ? minCustomRunningHeight : 0) - (historyExpanded ? minCustomHistoryHeight : 0))
	readonly property real customTodoHeight: Math.max(minCustomTodoHeight, Math.min(customTodoMax, Workspace.settings.sidebarTodoHeight))
	readonly property real todoExpandedHeight: (!runningExpanded && !historyExpanded)
		? Math.max(minCustomTodoHeight, bodySpace)
		: (Workspace.settings.sidebarTodoHeight > 0
			? customTodoHeight
			: Math.min(Theme.sidebarSectionHeight + Math.max(todoList.height, minTodoBodyHeight), todoCap))
	readonly property real todoHeight: !todoFits ? 0 : (todoExpanded ? todoExpandedHeight : Theme.sidebarSectionHeight)
	readonly property real remainingAfterTodo: Math.max(0, bodySpace - todoHeight)
	readonly property real runningMaxHeight: Math.min(remainingAfterTodo, Math.max(remainingAfterTodo - reservedHistory, minRunningHeight))
	readonly property real runningContentHeight: running.count > 0 ? running.count * Theme.sidebarRowHeight + Math.max(0, running.count - 1) * runningRowSpacing : 0
	readonly property real runningAutoHeight: Math.max(0, Math.min(runningContentHeight, runningMaxHeight))
	readonly property real customRunningMax: Math.max(minCustomRunningHeight, remainingAfterTodo - minCustomHistoryHeight)
	readonly property real runningTargetHeight: {
		if (!runningExpanded)
			return 0
		if (!historyExpanded)
			return remainingAfterTodo
		if (Workspace.settings.sidebarRunningHeight > 0)
			return Math.max(minCustomRunningHeight, Math.min(customRunningMax, Workspace.settings.sidebarRunningHeight))
		return runningAutoHeight
	}
	readonly property real historySpace: Math.max(0, bodySpace - todoHeight - runningTargetHeight)
	readonly property real historyTargetHeight: historyExpanded ? historySpace : 0
	readonly property bool historyFilterActive: Workspace.historyCreatorFilter !== "all" || Workspace.historyCurrentDir
	readonly property string emptyHistoryText: {
		if (!Workspace.historyLoaded)
			return qsTr("Loading…")
		return (searchInput.text !== "" || historyFilterActive) ? qsTr("No matching sessions") : qsTr("No earlier sessions")
	}
	readonly property int lockupWidth: 60 // the lockup is drawn at this size only: its pixels are aligned to it
	readonly property int lockupHeight: 24
	readonly property int lockupLeftMargin: 18
	property int headerLeftInset: 0
	property bool dragEnabled: true
	function keepTodo() {
		if (todo.items.length > 0 && JSON.stringify(todo) !== JSON.stringify(shownTodo))
			shownTodo = todo
	}

	property bool sectionAnimating: false

	Timer {
		id: sectionAnimTimer
		interval: Motion.standard + 60
		onTriggered: sidebar.sectionAnimating = false
	}

	function triggerSectionAnimation() {
		if (!ready)
			return
		sectionAnimating = true
		sectionAnimTimer.restart()
	}

	onTodoChanged: keepTodo()
	onTodoFitsChanged: triggerSectionAnimation()
	onTodoExpandedChanged: triggerSectionAnimation()
	onRunningExpandedChanged: triggerSectionAnimation()
	onHistoryExpandedChanged: {
		if (!historyExpanded && searchInput.activeFocus)
			searchInput.focus = false
		triggerSectionAnimation()
	}
	Component.onCompleted: {
		keepTodo()
		ready = true
	}
	color: Theme.surface
	clip: true

	signal collapseRequested()
	signal moveRequested()
	signal toggleMaximizeRequested()
	component SectionHeader: Item {
		id: sectionHeader

		required property string text
		required property bool expanded
		property bool ready: false
		signal toggled()

		width: parent.width
		height: Theme.sidebarSectionHeight
		activeFocusOnTab: true
		Accessible.role: Accessible.Button
		Accessible.name: text
		Accessible.onPressAction: toggled()
		Keys.onReturnPressed: event => {
			toggled()
			event.accepted = true
		}
		Keys.onEnterPressed: event => {
			toggled()
			event.accepted = true
		}
		Keys.onSpacePressed: event => {
			toggled()
			event.accepted = true
		}

		Rectangle {
			anchors { fill: parent; leftMargin: Theme.sidebarPadding; rightMargin: Theme.sidebarPadding }
			radius: Theme.radiusSmall
			color: headerHover.hovered ? Theme.surfaceHover : "transparent"
			border.width: 1
			border.color: sectionHeader.activeFocus ? Theme.accentText : "transparent"

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}
		Text {
			textFormat: Text.PlainText
			anchors { left: parent.left; leftMargin: Theme.listLabelInset; right: chevron.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
			text: sectionHeader.text
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			elide: Text.ElideRight
		}

		Chevron {
			id: chevron
			anchors { right: parent.right; rightMargin: 16; verticalCenter: parent.verticalCenter }
			color: Theme.textMuted
			rotation: sectionHeader.expanded ? 90 : 0

			Behavior on rotation {
				enabled: sectionHeader.ready
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}
		}

		HoverHandler {
			id: headerHover
			cursorShape: Qt.PointingHandCursor
		}
		TapHandler {
			onTapped: sectionHeader.toggled()
		}
	}

	component FilterMenuItem: Item {
		id: menuItem

		required property string text
		required property bool checked
		readonly property int textInset: 12
		readonly property int checkSize: 16
		signal triggered()

		width: parent ? parent.width : 196
		height: 30

		Rectangle {
			anchors.fill: parent
			radius: Theme.radiusSmall + 2
			color: itemMouse.containsMouse ? Theme.surfaceHover : "transparent"

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		Icon {
			id: checkIcon
			anchors { left: parent.left; leftMargin: menuItem.textInset; verticalCenter: parent.verticalCenter }
			name: "check"
			size: menuItem.checkSize
			color: Theme.accentText
			visible: menuItem.checked
		}

		Text {
			textFormat: Text.PlainText
			anchors {
				left: parent.left
				leftMargin: menuItem.textInset + menuItem.checkSize + 8
				right: parent.right
				rightMargin: menuItem.textInset
				verticalCenter: parent.verticalCenter
			}
			text: menuItem.text
			color: menuItem.checked ? Theme.accentText : Theme.text
			font.pixelSize: Theme.secondaryFontSize
			elide: Text.ElideRight
		}

		MouseArea {
			id: itemMouse
			anchors.fill: parent
			hoverEnabled: true
			cursorShape: Qt.PointingHandCursor
			onClicked: menuItem.triggered()
		}
	}
	Rectangle {
		anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
		width: 1
		color: resizeHover.hovered || resizeDrag.active ? Theme.accentText : Theme.border
		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
	Item {
		id: resizeHandle
		objectName: "sidebarResizeHandle"
		anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
		width: 6
		z: 10
		visible: Workspace.settings.sidebarVisible && sidebar.width >= Theme.sidebarMinWidth
		property real startWidth: sidebar.contentWidth
		property real startSceneX: 0

		function updateWidth(sceneX) {
			const nextWidth = resizeHandle.startWidth + (sceneX - resizeHandle.startSceneX) / UiScale.factor
			Workspace.settings.sidebarWidth = Math.round(Math.max(Theme.sidebarMinWidth, Math.min(sidebar.maxAllowed, nextWidth)))
		}

		HoverHandler {
			id: resizeHover
			cursorShape: Qt.SplitHCursor
		}
		DragHandler {
			id: resizeDrag
			enabled: sidebar.dragEnabled && Workspace.settings.sidebarVisible
			target: null
			yAxis.enabled: false
			dragThreshold: 2
			grabPermissions: PointerHandler.CanTakeOverFromAnything
			onActiveChanged: {
				if (active) {
					resizeHandle.startWidth = sidebar.contentWidth
					resizeHandle.startSceneX = centroid.scenePressPosition.x
					resizeHandle.updateWidth(centroid.scenePosition.x)
				}
			}
			onCentroidChanged: {
				if (active)
					resizeHandle.updateWidth(centroid.scenePosition.x)
			}
		}
		TapHandler {
			enabled: sidebar.dragEnabled && Workspace.settings.sidebarVisible
			gesturePolicy: TapHandler.ReleaseWithinBounds
			grabPermissions: PointerHandler.CanTakeOverFromAnything | PointerHandler.ApprovesTakeOverByHandlersOfDifferentType
			onDoubleTapped: Workspace.settings.sidebarWidth = Theme.sidebarWidth
		}
	}
	Item {
		id: column
		anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
		width: sidebar.contentWidth

		Item {
			id: header
			width: parent.width
			height: Theme.sidebarHeaderHeight

			Item {
				anchors { left: parent.left; right: collapseButton.left; top: parent.top; bottom: parent.bottom }

				DragHandler {
					enabled: sidebar.dragEnabled
					target: null
					dragThreshold: 4
					onActiveChanged: if (active) sidebar.moveRequested()
				}
				TapHandler {
					enabled: sidebar.dragEnabled
					onDoubleTapped: sidebar.toggleMaximizeRequested()
				}
			}

			Image {
				x: sidebar.headerLeftInset > 0 ? sidebar.headerLeftInset : sidebar.lockupLeftMargin
				y: (parent.height - height) / 2
				width: sidebar.lockupWidth
				height: sidebar.lockupHeight
				sourceSize: Qt.size(Math.max(1, Math.round(width * UiScale.factor)), Math.max(1, Math.round(height * UiScale.factor)))
				source: "qrc:/app/ref-lockup-small-" + (Theme.dark ? ThemeName.dark : ThemeName.light) + ".svg"
				smooth: UiScale.factor !== 1
				Accessible.role: Accessible.Graphic
				Accessible.name: "ref"
			}
			IconButton {
				id: collapseButton
				anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
				buttonSize: 32
				iconName: "sidebar"
				Accessible.name: qsTr("Hide sidebar")
				onClicked: sidebar.collapseRequested()
			}
		}

		Item {
			id: newRow
			anchors.top: header.bottom
			width: parent.width
			height: sidebar.newRowHeight

			IconButton {
				id: stashButton
				objectName: "sidebarStashButton"
				anchors { left: parent.left; leftMargin: Theme.sidebarPadding; verticalCenter: parent.verticalCenter }
				buttonSize: 36
				iconSize: 20
				iconName: Workspace.stash.groupCount > 0 ? "stash-count" : "stash"
				badgeText: Workspace.stash.groupCount > 9 ? "9+" : (Workspace.stash.groupCount > 0 ? String(Workspace.stash.groupCount) : "")
				enabled: Workspace.connectionState === "connected"
				opacity: enabled ? 1 : Theme.disabledOpacity
				tooltipText: Workspace.sessions.count > 0
					? qsTr("Stash all running sessions (right-click to manage)")
					: qsTr("Manage stashed groups")
				Accessible.name: Workspace.sessions.count > 0 ? qsTr("Stash all running sessions") : qsTr("Manage stashed groups")
				onClicked: {
					if (!stashButton.enabled)
						return
					if (Workspace.sessions.count > 0)
						Workspace.stash.stashAll()
					else
						Workspace.openDialog(DialogId.stash)
				}

				TapHandler {
					enabled: stashButton.enabled
					acceptedButtons: Qt.RightButton
					onTapped: Workspace.openDialog(DialogId.stash)
				}
			}
			Item {
				id: newButton
				objectName: "sidebarNewSessionButton"
				anchors { left: stashButton.right; leftMargin: 8; right: folderButton.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
				height: 36

				readonly property real halfW: Math.max(1, width * 0.5)
				readonly property real halfH: Math.max(1, height * 0.5)
				readonly property real cursorNx: newHover.hovered ? Math.max(-1, Math.min(1, (newHover.point.position.x - halfW) / halfW)) : 0
				readonly property real cursorNy: newHover.hovered ? Math.max(-1, Math.min(1, (newHover.point.position.y - halfH) / halfH)) : 0
				readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)
				property real glowStrength: newHover.hovered ? 1.0 : 0.0
				property real glowX: newHover.hovered ? newHover.point.position.x : width * 0.5
				property real glowY: newHover.hovered ? newHover.point.position.y : height * 0.5

				Behavior on glowStrength { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on glowX { SpringAnimation { spring: 4.6; damping: 0.36; epsilon: 0.2 } }
				Behavior on glowY { SpringAnimation { spring: 4.6; damping: 0.36; epsilon: 0.2 } }

				function cssRgba(c, alpha) {
					const r = Math.round(c.r * 255)
					const g = Math.round(c.g * 255)
					const b = Math.round(c.b * 255)
					const a = Math.max(0, Math.min(1, alpha))
					return "rgba(" + r + "," + g + "," + b + "," + a.toFixed(4) + ")"
				}
				function scheduleNewGlow() {
					if (newGlowCanvas.visible)
						newGlowCanvas.requestPaint()
				}

				onGlowStrengthChanged: scheduleNewGlow()
				onGlowXChanged: scheduleNewGlow()
				onGlowYChanged: scheduleNewGlow()
				onPixelScaleChanged: {
					scheduleNewGlow()
					chatPlusCanvas.requestPaint()
				}

				Connections {
					target: Theme
					function onDarkChanged() {
						newButton.scheduleNewGlow()
						chatPlusCanvas.requestPaint()
					}
				}

				Rectangle {
					id: newFace
					anchors.fill: parent
					radius: 18
					color: newHover.hovered ? Theme.surfaceHover : Theme.surface
					border.color: newHover.hovered
						? Qt.tint(Theme.border, Qt.alpha(Theme.success, Theme.dark ? 0.50 : 0.58))
						: Theme.border
					scale: newTap.pressed ? 0.96 : (newHover.hovered ? 1.02 : 1.0)

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					Behavior on scale { SpringAnimation { spring: 5.2; damping: 0.30; epsilon: 0.002 } }

					transform: Translate {
						x: newHover.hovered ? newButton.cursorNx * 1.0 : 0
						y: newHover.hovered ? newButton.cursorNy * 0.6 : 0

						Behavior on x { SpringAnimation { spring: 4.2; damping: 0.34; epsilon: 0.05 } }
						Behavior on y { SpringAnimation { spring: 4.2; damping: 0.34; epsilon: 0.05 } }
					}

					// Restrained creation-green pointer-following radial glow clipped cleanly inside the capsule.
					Canvas {
						id: newGlowCanvas
						x: 0
						y: 0
						width: Math.max(1, Math.ceil(newFace.width * newButton.pixelScale))
						height: Math.max(1, Math.ceil(newFace.height * newButton.pixelScale))
						transformOrigin: Item.TopLeft
						scale: 1 / newButton.pixelScale
						renderTarget: Canvas.Image
						visible: newButton.glowStrength > 0.005

						onVisibleChanged: if (visible) requestPaint()
						onWidthChanged: if (visible) requestPaint()
						onHeightChanged: if (visible) requestPaint()

						onPaint: {
							const ctx = getContext("2d")
							if (!ctx)
								return
							ctx.setTransform(1, 0, 0, 1, 0, 0)
							ctx.clearRect(0, 0, width, height)
							if (!visible || newFace.width <= 0 || newFace.height <= 0)
								return
							ctx.setTransform(newButton.pixelScale, 0, 0, newButton.pixelScale, 0, 0)

							const w = newFace.width
							const h = newFace.height
							const r = Math.min(17.5, h * 0.5 - 0.5)
							ctx.save()
							ctx.beginPath()
							ctx.moveTo(0.5 + r, 0.5)
							ctx.lineTo(w - 0.5 - r, 0.5)
							ctx.arcTo(w - 0.5, 0.5, w - 0.5, 0.5 + r, r)
							ctx.lineTo(w - 0.5, h - 0.5 - r)
							ctx.arcTo(w - 0.5, h - 0.5, w - 0.5 - r, h - 0.5, r)
							ctx.lineTo(0.5 + r, h - 0.5)
							ctx.arcTo(0.5, h - 0.5, 0.5, h - 0.5 - r, r)
							ctx.lineTo(0.5, 0.5 + r)
							ctx.arcTo(0.5, 0.5, 0.5 + r, 0.5, r)
							ctx.closePath()
							ctx.clip()

							const gx = newButton.glowX
							const gy = newButton.glowY
							const gr = Math.max(56, w * 0.42)
							const a = (Theme.dark ? 0.18 : 0.14) * newButton.glowStrength
							const grad = ctx.createRadialGradient(gx, gy, 0, gx, gy, gr)
							grad.addColorStop(0.0, newButton.cssRgba(Theme.success, a))
							grad.addColorStop(0.55, newButton.cssRgba(Theme.success, a * 0.34))
							grad.addColorStop(1.0, newButton.cssRgba(Theme.success, 0))
							ctx.fillStyle = grad
							ctx.fillRect(0, 0, w, h)
							ctx.restore()
						}
					}

					Row {
						anchors.centerIn: parent
						spacing: 8

						// Multi-part HiDPI Canvas chat-plus (1:1 assets/icons/chat-plus-16.svg).
						Item {
							id: chatPlusHost
							anchors.verticalCenter: parent.verticalCenter
							width: 16
							height: 16
							readonly property real pad: 3
							property real hoverVal: newTap.pressed ? -0.35 : (newHover.hovered ? 1.0 : 0.0)
							property real plusRot: newHover.hovered ? 90.0 : 0.0
							property color bubbleColor: Theme.text
							property color plusColor: newHover.hovered ? Theme.successText : Theme.text

							Behavior on hoverVal { SpringAnimation { spring: 5.0; damping: 0.28; epsilon: 0.004 } }
							Behavior on plusRot { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
							Behavior on plusColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							onHoverValChanged: chatPlusCanvas.requestPaint()
							onPlusRotChanged: chatPlusCanvas.requestPaint()
							onBubbleColorChanged: chatPlusCanvas.requestPaint()
							onPlusColorChanged: chatPlusCanvas.requestPaint()

							Canvas {
								id: chatPlusCanvas
								x: -chatPlusHost.pad
								y: -chatPlusHost.pad
								readonly property real logicalW: chatPlusHost.width + 2 * chatPlusHost.pad
								readonly property real logicalH: chatPlusHost.height + 2 * chatPlusHost.pad
								width: Math.max(1, Math.ceil(logicalW * newButton.pixelScale))
								height: Math.max(1, Math.ceil(logicalH * newButton.pixelScale))
								transformOrigin: Item.TopLeft
								scale: 1 / newButton.pixelScale
								renderTarget: Canvas.Image

								onWidthChanged: requestPaint()
								onHeightChanged: requestPaint()
								Component.onCompleted: requestPaint()

								onPaint: {
									const ctx = getContext("2d")
									if (!ctx || chatPlusHost.width <= 0 || chatPlusHost.height <= 0)
										return
									ctx.setTransform(1, 0, 0, 1, 0, 0)
									ctx.clearRect(0, 0, width, height)
									const ps = newButton.pixelScale
									const s = chatPlusHost.width / 16.0
									ctx.setTransform(ps, 0, 0, ps, 0, 0)
									ctx.translate(chatPlusHost.pad, chatPlusHost.pad)
									ctx.scale(s, s)

									const hv = chatPlusHost.hoverVal
									const bScale = 1.0 + 0.07 * hv
									const bLift = -0.6 * hv
									ctx.lineWidth = 1.45
									ctx.lineCap = "round"
									ctx.lineJoin = "round"

									// 1. Chat bubble lifting and scaling gently on hover (exact chat-plus-16.svg).
									ctx.save()
									ctx.translate(8.0, 7.5 + bLift)
									ctx.scale(bScale, bScale)
									ctx.translate(-8.0, -7.5)
									ctx.strokeStyle = newButton.cssRgba(chatPlusHost.bubbleColor, 1.0)
									ctx.beginPath()
									ctx.moveTo(3.75, 2.25)
									ctx.lineTo(12.25, 2.25)
									ctx.arcTo(13.75, 2.25, 13.75, 3.75, 1.5)
									ctx.lineTo(13.75, 9.75)
									ctx.arcTo(13.75, 11.25, 12.25, 11.25, 1.5)
									ctx.lineTo(8.25, 11.25)
									ctx.lineTo(5.75, 13.75)
									ctx.lineTo(5.75, 11.25)
									ctx.lineTo(3.75, 11.25)
									ctx.arcTo(2.25, 11.25, 2.25, 9.75, 1.5)
									ctx.lineTo(2.25, 3.75)
									ctx.arcTo(2.25, 2.25, 3.75, 2.25, 1.5)
									ctx.closePath()
									ctx.stroke()

									// 2. Inner '+' rotating +90° clockwise, elongating & lighting up in Theme.successText.
									const halfArm = 2.0 + 0.32 * Math.max(0, hv)
									ctx.translate(8.25, 6.75)
									ctx.rotate(chatPlusHost.plusRot * Math.PI / 180)
									ctx.strokeStyle = newButton.cssRgba(chatPlusHost.plusColor, 1.0)
									ctx.beginPath()
									ctx.moveTo(0, -halfArm)
									ctx.lineTo(0, halfArm)
									ctx.moveTo(-halfArm, 0)
									ctx.lineTo(halfArm, 0)
									ctx.stroke()
									ctx.restore()
								}
							}
						}
						Text {
							textFormat: Text.PlainText
							anchors.verticalCenter: parent.verticalCenter
							text: qsTr("New session")
							color: Theme.text
							font.pixelSize: Theme.captionFontSize
						}
					}
				}
				HoverHandler {
					id: newHover
					cursorShape: Qt.PointingHandCursor
				}
				TapHandler {
					id: newTap
					onTapped: Commands.run(CommandId.newSession)
				}
			}
			IconButton {
				id: folderButton
				anchors { right: parent.right; rightMargin: Theme.sidebarPadding; verticalCenter: parent.verticalCenter }
				buttonSize: 36
				iconName: "folder-open"
				Accessible.name: qsTr("New session in another folder…")
				onClicked: Commands.run(CommandId.newSessionIn)
			}
		}

		// The to-do list of the session in front, as the todo call's card shows it. Hidden without items.
		Item {
			id: todoSection
			objectName: "todoSection"
			anchors.top: newRow.bottom
			width: parent.width
			height: sidebar.todoHeight
			clip: true
			opacity: sidebar.todoFits ? 1 : 0
			visible: height > 0

			Behavior on height {
				enabled: sidebar.ready && sidebar.sectionAnimating && !sidebar.resizingSections
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}
			Behavior on opacity {
				enabled: sidebar.ready
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}

			SectionHeader {
				id: todoHeader
				objectName: "todoSectionHeader"
				text: qsTr("Todo %1/%2").arg(sidebar.shownTodo.done).arg(sidebar.shownTodo.total)
				expanded: sidebar.todoExpanded
				ready: sidebar.ready
				onToggled: {
					sidebar.triggerSectionAnimation()
					Workspace.settings.sidebarTodoExpanded = !Workspace.settings.sidebarTodoExpanded
				}
			}

			Flickable {
				id: todoFlickable
				objectName: "todoListFlickable"
				y: todoHeader.height
				width: parent.width
				height: Math.max(0, parent.height - todoHeader.height)
				contentWidth: width
				contentHeight: todoList.height
				boundsBehavior: Flickable.StopAtBounds
				clip: true
				opacity: sidebar.todoExpanded ? 1 : 0
				visible: height > 0 && opacity > 0
				interactive: sidebar.todoExpanded

				Behavior on opacity {
					enabled: sidebar.ready
					NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
				}

				TodoList {
					id: todoList
					width: parent.width
					items: sidebar.shownTodo.items
					wrap: true
					leftPad: Theme.listLabelInset // in line with the label
				}

				ScrollBar.vertical: ThemedScrollBar {}
			}
		}

		Item {
			id: todoResizeHandle
			objectName: "todoResizeHandle"
			anchors { left: parent.left; right: parent.right; verticalCenter: todoSection.bottom }
			height: 6
			z: 5
			visible: sidebar.todoFits && sidebar.todoExpanded && (sidebar.runningExpanded || sidebar.historyExpanded) && Workspace.settings.sidebarVisible
			property real startHeight: todoSection.height
			property real startSceneY: 0

			function updateHeight(sceneY) {
				const raw = todoResizeHandle.startHeight + (sceneY - todoResizeHandle.startSceneY) / UiScale.factor
				Workspace.settings.sidebarTodoHeight = Math.round(Math.max(sidebar.minCustomTodoHeight, Math.min(sidebar.customTodoMax, raw)))
			}

			Rectangle {
				anchors.verticalCenter: parent.verticalCenter
				width: parent.width
				height: 2
				color: Theme.accentText
				opacity: todoResizeHover.hovered || todoResizeDrag.active ? 1 : 0

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
			HoverHandler {
				id: todoResizeHover
				cursorShape: Qt.SplitVCursor
			}
			DragHandler {
				id: todoResizeDrag
				enabled: sidebar.dragEnabled && todoResizeHandle.visible
				target: null
				xAxis.enabled: false
				dragThreshold: 2
				grabPermissions: PointerHandler.CanTakeOverFromAnything
				onActiveChanged: {
					if (active) {
						todoResizeHandle.startHeight = todoSection.height
						todoResizeHandle.startSceneY = centroid.scenePressPosition.y
						todoResizeHandle.updateHeight(centroid.scenePosition.y)
					}
				}
				onCentroidChanged: {
					if (active)
						todoResizeHandle.updateHeight(centroid.scenePosition.y)
				}
			}
			TapHandler {
				enabled: sidebar.dragEnabled && todoResizeHandle.visible
				gesturePolicy: TapHandler.ReleaseWithinBounds
				grabPermissions: PointerHandler.CanTakeOverFromAnything | PointerHandler.ApprovesTakeOverByHandlersOfDifferentType
				onDoubleTapped: Workspace.settings.sidebarTodoHeight = 0
			}
		}

		SectionHeader {
			id: runningHeader
			objectName: "runningSectionHeader"
			anchors.top: todoSection.bottom
			text: qsTr("Running")
			expanded: sidebar.runningExpanded
			ready: sidebar.ready
			onToggled: {
				sidebar.triggerSectionAnimation()
				Workspace.settings.sidebarRunningExpanded = !Workspace.settings.sidebarRunningExpanded
			}
		}

		ListView {
			id: running
			objectName: "runningList"
			anchors.top: runningHeader.bottom
			width: parent.width
			height: sidebar.runningTargetHeight
			visible: height > 0
			enabled: sidebar.runningExpanded && Workspace.settings.sidebarVisible
			model: Workspace.sessions
			spacing: sidebar.runningRowSpacing
			clip: true
			boundsBehavior: Flickable.StopAtBounds
			interactive: !sidebar.reorderingRunning
			delegate: SessionRow {
				anyDragging: sidebar.reorderingRunning
				pinnedCount: Workspace.sessions.pinnedCount
				onDragActiveChanged: (sessionId, active) => {
					if (active)
						sidebar.draggingSessionId = sessionId
					else if (sidebar.draggingSessionId === sessionId)
						sidebar.draggingSessionId = ""
				}
				onReorderRequested: (sessionId, toIndex) => Workspace.moveRunningSession(sessionId, toIndex)
			}

			Behavior on height {
				enabled: sidebar.ready && sidebar.sectionAnimating && !sidebar.resizingSections
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}
			add: Transition {
				ParallelAnimation {
					NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
					NumberAnimation { property: "scale"; from: Motion.popScale - 0.01; to: 1; duration: Motion.standard; easing.type: Motion.ease }
				}
			}
			remove: Transition {
				ParallelAnimation {
					NumberAnimation { property: "opacity"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
					NumberAnimation { property: "scale"; to: Motion.tileExitScale; duration: Motion.standard; easing.type: Motion.exitEase }
				}
			}
			// A row displaced while it enters is not finished by the add transition: it ends at full opacity and scale here.
			displaced: Transition {
				NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
				NumberAnimation { property: "opacity"; to: 1; duration: Motion.standard; easing.type: Motion.ease }
				NumberAnimation { property: "scale"; to: 1; duration: Motion.standard; easing.type: Motion.ease }
			}
			move: Transition {
				NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
			}
			moveDisplaced: Transition {
				NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
			}

			ScrollBar.vertical: ThemedScrollBar {}
		}

		Item {
			id: runningResizeHandle
			objectName: "runningResizeHandle"
			anchors { left: parent.left; right: parent.right; verticalCenter: running.bottom }
			height: 6
			z: 5
			visible: sidebar.runningExpanded && sidebar.historyExpanded && Workspace.settings.sidebarVisible
			property real startHeight: running.height
			property real startSceneY: 0

			function updateHeight(sceneY) {
				const raw = runningResizeHandle.startHeight + (sceneY - runningResizeHandle.startSceneY) / UiScale.factor
				Workspace.settings.sidebarRunningHeight = Math.round(Math.max(sidebar.minCustomRunningHeight, Math.min(sidebar.customRunningMax, raw)))
			}

			Rectangle {
				anchors.verticalCenter: parent.verticalCenter
				width: parent.width
				height: 2
				color: Theme.accentText
				opacity: runningResizeHover.hovered || runningResizeDrag.active ? 1 : 0

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
			HoverHandler {
				id: runningResizeHover
				cursorShape: Qt.SplitVCursor
			}
			DragHandler {
				id: runningResizeDrag
				enabled: sidebar.dragEnabled && runningResizeHandle.visible
				target: null
				xAxis.enabled: false
				dragThreshold: 2
				grabPermissions: PointerHandler.CanTakeOverFromAnything
				onActiveChanged: {
					if (active) {
						runningResizeHandle.startHeight = running.height
						runningResizeHandle.startSceneY = centroid.scenePressPosition.y
						runningResizeHandle.updateHeight(centroid.scenePosition.y)
					}
				}
				onCentroidChanged: {
					if (active)
						runningResizeHandle.updateHeight(centroid.scenePosition.y)
				}
			}
			TapHandler {
				enabled: sidebar.dragEnabled && runningResizeHandle.visible
				gesturePolicy: TapHandler.ReleaseWithinBounds
				grabPermissions: PointerHandler.CanTakeOverFromAnything | PointerHandler.ApprovesTakeOverByHandlersOfDifferentType
				onDoubleTapped: Workspace.settings.sidebarRunningHeight = 0
			}
		}

		SectionHeader {
			id: historyHeader
			objectName: "historySectionHeader"
			anchors.top: running.bottom
			text: qsTr("History")
			expanded: sidebar.historyExpanded
			ready: sidebar.ready
			onToggled: {
				sidebar.triggerSectionAnimation()
				Workspace.settings.sidebarHistoryExpanded = !Workspace.settings.sidebarHistoryExpanded
			}
		}

		Item {
			id: historyBody
			objectName: "historyBody"
			anchors.top: historyHeader.bottom
			width: parent.width
			height: sidebar.historyTargetHeight
			clip: true
			visible: height > 0
			enabled: sidebar.historyExpanded && Workspace.settings.sidebarVisible

			Behavior on height {
				enabled: sidebar.ready && sidebar.sectionAnimating && !sidebar.resizingSections
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}

			// Keep inner geometry at full height so search and history never squeeze below zero during fold.
			Item {
				width: parent.width
				height: Math.max(sidebar.historyOverhead, sidebar.historyTargetHeight, historyBody.height)

				// The search field of the history.
				Rectangle {
					id: search
					anchors.top: parent.top
					x: Theme.sidebarPadding
					width: parent.width - 2 * Theme.sidebarPadding
					height: sidebar.searchHeight
					radius: 16
					color: Theme.surface
					border.color: searchInput.activeFocus ? Theme.accentText : Theme.border

					Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

					Icon {
						x: 10
						anchors.verticalCenter: parent.verticalCenter
						name: "search"
						size: 16
						color: Theme.textMuted
					}
					TextInput {
						id: searchInput
						anchors {
							left: parent.left
							leftMargin: 34
							right: clear.visible ? clear.left : filterButton.left
							rightMargin: 6
							verticalCenter: parent.verticalCenter
						}
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font.pixelSize: Theme.captionFontSize
						clip: true
						Accessible.name: qsTr("Search history")
						onTextChanged: Workspace.setHistorySearch(text)

						PlaceholderText {
							anchors.fill: parent
							textInput: searchInput
							text: qsTr("Search history")
							font.pixelSize: Theme.captionFontSize
						}
					}
					IconButton {
						id: clear
						anchors { right: filterButton.left; rightMargin: 2; verticalCenter: parent.verticalCenter }
						buttonSize: 22
						iconName: "close"
						iconSize: 16
						opacity: searchInput.text !== "" ? 1 : 0
						visible: opacity > 0
						Accessible.name: qsTr("Clear search")
						onClicked: searchInput.text = ""

						Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					}
					IconButton {
						id: filterButton
						objectName: "historyFilterButton"
						anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
						buttonSize: 22
						iconName: "filter"
						iconSize: 16
						iconColor: sidebar.historyFilterActive ? Theme.accentText : Theme.textMuted
						tooltipText: qsTr("Filter history")
						Accessible.name: qsTr("Filter history")
						onClicked: historyFilterMenu.visible ? historyFilterMenu.close() : historyFilterMenu.open()

						Popup {
							id: historyFilterMenu
							objectName: "historyFilterMenu"
							readonly property real uiScale: UiScale.factor
							readonly property int menuPadding: 4
							readonly property int menuWidth: 204

							parent: filterButton
							x: filterButton.width - width
							y: filterButton.height + 6
							width: menuInner.width * uiScale
							height: menuInner.height * uiScale
							padding: 0
							modal: true
							focus: true
							Overlay.modal: Rectangle { color: "transparent" }
							closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
							background: null

							enter: Transition {
								NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.quick; easing.type: Motion.ease }
								NumberAnimation { property: "scale"; from: Motion.popScale; to: 1; duration: Motion.quick; easing.type: Motion.ease }
							}
							exit: Transition {
								NumberAnimation { property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
							}

							contentItem: Item {
								// Swallow clicks inside the popup padding so they cannot bleed through to rows underneath.
								MouseArea {
									anchors.fill: parent
								}

								Item {
									id: menuInner
									width: historyFilterMenu.menuWidth
									height: menuColumn.implicitHeight + 2 * historyFilterMenu.menuPadding
									transformOrigin: Item.TopLeft
									scale: historyFilterMenu.uiScale

									Rectangle {
										anchors.fill: parent
										radius: Theme.radius
										color: Theme.surface
										border.color: Theme.border
									}

									Column {
										id: menuColumn
										x: historyFilterMenu.menuPadding
										y: historyFilterMenu.menuPadding
										width: parent.width - 2 * historyFilterMenu.menuPadding
										spacing: 2

										FilterMenuItem {
											objectName: "historyFilterAll"
											text: qsTr("All")
											checked: Workspace.historyCreatorFilter === "all"
											onTriggered: {
												Workspace.setHistoryFilter("all", Workspace.historyCurrentDir)
												historyFilterMenu.close()
											}
										}
										FilterMenuItem {
											objectName: "historyFilterUser"
											text: qsTr("User-created")
											checked: Workspace.historyCreatorFilter === "user"
											onTriggered: {
												Workspace.setHistoryFilter("user", Workspace.historyCurrentDir)
												historyFilterMenu.close()
											}
										}
										FilterMenuItem {
											objectName: "historyFilterCreator"
											visible: Workspace.historyCreatorFilter === "creator" && Workspace.historyCreatorId !== ""
											text: qsTr("By %1").arg(Workspace.historyCreatorLabel)
											checked: Workspace.historyCreatorFilter === "creator"
											onTriggered: historyFilterMenu.close()
										}
										Rectangle {
											width: parent.width
											height: 1
											color: Theme.border
										}
										FilterMenuItem {
											objectName: "historyFilterCurrentDir"
											text: qsTr("Only current directory")
											checked: Workspace.historyCurrentDir
											onTriggered: {
												Workspace.setHistoryFilter(Workspace.historyCreatorFilter, !Workspace.historyCurrentDir, Workspace.historyCreatorId)
												historyFilterMenu.close()
											}
										}
									}
								}
							}
						}
					}
				}

				ListView {
					id: history
					objectName: "historyList"
					anchors { top: search.bottom; topMargin: sidebar.historyGap; bottom: parent.bottom }
					width: parent.width
					model: Workspace.history
					clip: true
					boundsBehavior: Flickable.StopAtBounds
					delegate: HistoryRow {}

					ScrollBar.vertical: ThemedScrollBar {}

					Text {
						textFormat: Text.PlainText
						anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 16 }
						visible: history.count === 0
						text: sidebar.emptyHistoryText
						color: Theme.textMuted
						font.pixelSize: Theme.captionFontSize
					}
				}
			}
		}

		Item {
			id: footerBar
			anchors.bottom: parent.bottom
			width: parent.width
			height: Theme.sidebarFooterHeight

			Rectangle {
				width: parent.width
				height: 1
				color: Theme.border
			}

			McpSummary {
				anchors { left: parent.left; leftMargin: 18; verticalCenter: parent.verticalCenter }
			}

			Row {
				id: footerActions
				anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 4

				IconButton {
					buttonSize: 32
					iconName: "retry"
					iconSize: 16
					enabled: Workspace.connectionState === "connected"
					tooltipText: qsTr("Reload configuration")
					Accessible.name: qsTr("Reload configuration")
					onClicked: Commands.run(CommandId.reload)
				}
				IconButton {
					buttonSize: 32
					iconName: "file-text"
					iconSize: 16
					enabled: Workspace.connectionState === "connected"
					tooltipText: qsTr("Configuration (ref.json)…")
					Accessible.name: qsTr("Configuration (ref.json)…")
					onClicked: Commands.run(CommandId.config)
				}
				IconButton {
					buttonSize: 32
					iconName: "sliders"
					tooltipText: qsTr("Settings")
					Accessible.name: qsTr("Settings")
					onClicked: Commands.run(CommandId.settings)
				}
			}
		}
	}
}
