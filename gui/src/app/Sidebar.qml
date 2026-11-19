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
	readonly property real reservedRunning: running.openProgress * minRunningHeight
	readonly property real reservedHistory: historyBody.openProgress * (historyOverhead + minHistoryHeight)
	property bool ready: false

	// The to-do list of the session in front: the current one, or the focused tile in split view. One value, {items,
	// done, total}, and nothing else is derived from it by a binding that could be read before it has caught up. The
	// section keeps what it showed while it closes, and changes only when the list does.
	readonly property var front: Workspace.frontSession
	readonly property var todo: front ? front.todo : ({ items: [], done: 0, total: 0 })
	property var shownTodo: ({ items: [], done: 0, total: 0 })
	// Up to a third of the column, and never so much that expanded lists below lose their minimum.
	readonly property real todoCap: Math.max(0, Math.min(height / 3, bodySpace - reservedRunning - reservedHistory))
	readonly property bool todoFits: todo.items.length > 0 && todoCap >= Theme.sidebarSectionHeight + todoList.rowHeight
	readonly property real minTodoBodyHeight: todoList.rowHeight + 2 * todoList.verticalPadding
	readonly property real todoExpandedHeight: Math.min(Theme.sidebarSectionHeight + Math.max(todoList.height, minTodoBodyHeight), todoCap)
	readonly property real todoHeight: !todoFits ? 0 : (todoExpanded ? todoExpandedHeight : Theme.sidebarSectionHeight)
	readonly property real remainingAfterTodo: Math.max(0, bodySpace - todoSection.height)
	readonly property real runningMaxHeight: Math.min(remainingAfterTodo, Math.max(remainingAfterTodo - reservedHistory, minRunningHeight))
	readonly property real runningTargetHeight: Math.max(0, Math.min(running.contentHeight, runningMaxHeight))
	readonly property real historySpace: Math.max(0, bodySpace - todoSection.height - running.height)
	readonly property string emptyHistoryText: {
		if (!Workspace.historyLoaded)
			return qsTr("Loading…")
		return searchInput.text !== "" ? qsTr("No matching sessions") : qsTr("No earlier sessions")
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

	onTodoChanged: keepTodo()
	onHistoryExpandedChanged: {
		if (!historyExpanded && searchInput.activeFocus)
			searchInput.focus = false
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
			const nextWidth = resizeHandle.startWidth + (sceneX - resizeHandle.startSceneX)
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
				sourceSize: Qt.size(width, height)
				source: "qrc:/app/ref-lockup-small-" + (Theme.dark ? ThemeName.dark : ThemeName.light) + ".svg"
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

			Item {
				id: newButton
				x: Theme.sidebarPadding
				anchors.verticalCenter: parent.verticalCenter
				width: parent.width - 2 * Theme.sidebarPadding - folderButton.width - 8
				height: 36

				Rectangle {
					anchors.fill: parent
					radius: 18
					color: newHover.hovered ? Theme.surfaceHover : Theme.surface
					border.color: Theme.border
					scale: newTap.pressed ? 0.96 : 1

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					Row {
						anchors.centerIn: parent
						spacing: 8

						Icon {
							anchors.verticalCenter: parent.verticalCenter
							name: "chat-plus"
							size: 16
							color: Theme.text
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
				enabled: sidebar.ready
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
				onToggled: Workspace.settings.sidebarTodoExpanded = !Workspace.settings.sidebarTodoExpanded
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

		SectionHeader {
			id: runningHeader
			objectName: "runningSectionHeader"
			anchors.top: todoSection.bottom
			text: qsTr("Running")
			expanded: sidebar.runningExpanded
			ready: sidebar.ready
			onToggled: Workspace.settings.sidebarRunningExpanded = !Workspace.settings.sidebarRunningExpanded
		}

		ListView {
			id: running
			objectName: "runningList"
			property real openProgress: sidebar.runningExpanded ? 1 : 0
			anchors.top: runningHeader.bottom
			width: parent.width
			// The rows' own height; scrolls only when the history would get less than its minimum.
			height: openProgress === 1 ? sidebar.runningTargetHeight : Math.round(sidebar.runningTargetHeight * openProgress)
			visible: height > 0
			enabled: sidebar.runningExpanded
			model: Workspace.sessions
			spacing: sidebar.runningRowSpacing
			clip: true
			boundsBehavior: Flickable.StopAtBounds
			delegate: SessionRow {}

			Behavior on openProgress {
				enabled: sidebar.ready
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

			ScrollBar.vertical: ThemedScrollBar {}
		}

		SectionHeader {
			id: historyHeader
			objectName: "historySectionHeader"
			anchors.top: running.bottom
			text: qsTr("History")
			expanded: sidebar.historyExpanded
			ready: sidebar.ready
			onToggled: Workspace.settings.sidebarHistoryExpanded = !Workspace.settings.sidebarHistoryExpanded
		}

		Item {
			id: historyBody
			objectName: "historyBody"
			property real openProgress: sidebar.historyExpanded ? 1 : 0
			anchors.top: historyHeader.bottom
			width: parent.width
			height: openProgress === 1 ? sidebar.historySpace : Math.round(sidebar.historySpace * openProgress)
			clip: true
			visible: height > 0
			enabled: sidebar.historyExpanded

			Behavior on openProgress {
				enabled: sidebar.ready
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}

			// Keep inner geometry at full height so search and history never squeeze below zero during fold.
			Item {
				width: parent.width
				height: Math.max(sidebar.historyOverhead, sidebar.historySpace)

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
						anchors { left: parent.left; leftMargin: 34; right: clear.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font.pixelSize: Theme.captionFontSize
						clip: true
						Accessible.name: qsTr("Search history")
						onTextChanged: Workspace.history.filter = text.trim()

						PlaceholderText {
							anchors.fill: parent
							textInput: searchInput
							text: qsTr("Search history")
							font.pixelSize: Theme.captionFontSize
						}
					}
					IconButton {
						id: clear
						anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
						buttonSize: 22
						iconName: "close"
						iconSize: 16
						opacity: searchInput.text !== "" ? 1 : 0
						visible: opacity > 0
						Accessible.name: qsTr("Clear search")
						onClicked: searchInput.text = ""

						Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
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
