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
	// Space for the three lists: what is left of the column after the fixed parts.
	readonly property real listSpace: fixedSpace - todoSection.height
	readonly property real fixedSpace: height - Theme.sidebarHeaderHeight - newRowHeight - 2 * Theme.sidebarSectionHeight - searchHeight - historyGap - Theme.sidebarFooterHeight

	// The to-do list of the session in front: the current one, or the focused tile in split view. One value, {items,
	// done, total}, and nothing else is derived from it by a binding that could be read before it has caught up. The
	// section keeps what it showed while it closes, and changes only when the list does.
	readonly property var front: Workspace.frontSession
	readonly property var todo: front ? front.todo : ({ items: [], done: 0, total: 0 })
	property var shownTodo: ({ items: [], done: 0, total: 0 })
	// Up to a third of the column, and never so much that the lists below lose their minimum.
	readonly property real todoCap: Math.min(height / 3, fixedSpace - minRunningHeight - minHistoryHeight)
	readonly property bool todoFits: todo.items.length > 0 && todoCap >= Theme.sidebarSectionHeight + todoList.rowHeight
	readonly property real todoHeight: todoFits ? Math.min(Theme.sidebarSectionHeight + todoList.height, todoCap) : 0
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
	Component.onCompleted: keepTodo()

	color: Theme.surface
	clip: true

	signal collapseRequested()
	signal moveRequested()
	signal toggleMaximizeRequested()
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
			onActiveChanged: {
				if (active)
					resizeHandle.startWidth = sidebar.contentWidth
			}
			onTranslationChanged: {
				if (active)
					Workspace.settings.sidebarWidth = Math.round(Math.max(Theme.sidebarMinWidth, Math.min(sidebar.maxAllowed, resizeHandle.startWidth + translation.x)))
			}
		}
		TapHandler {
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
			anchors.top: newRow.bottom
			width: parent.width
			height: sidebar.todoHeight
			clip: true
			opacity: sidebar.todo.items.length > 0 ? 1 : 0
			visible: height > 0

			Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			SectionLabel {
				id: todoLabel
				leftPadding: Theme.listLabelInset
				text: qsTr("Todo %1/%2").arg(sidebar.shownTodo.done).arg(sidebar.shownTodo.total)
			}

			Flickable {
				anchors { top: todoLabel.bottom; bottom: parent.bottom }
				width: parent.width
				contentWidth: width
				contentHeight: todoList.height
				boundsBehavior: Flickable.StopAtBounds
				clip: true

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

		SectionLabel {
			id: runningLabel
			leftPadding: Theme.listLabelInset
			anchors.top: todoSection.bottom
			text: qsTr("Running")
		}

		ListView {
			id: running
			anchors.top: runningLabel.bottom
			width: parent.width
			// The rows' own height; scrolls only when the history would get less than its minimum.
			height: Math.min(contentHeight, Math.max(sidebar.listSpace - sidebar.minHistoryHeight, sidebar.minRunningHeight))
			model: Workspace.sessions
			spacing: sidebar.runningRowSpacing
			clip: true
			boundsBehavior: Flickable.StopAtBounds
			delegate: SessionRow {}

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

		SectionLabel {
			id: historyLabel
			leftPadding: Theme.listLabelInset
			anchors.top: running.bottom
			text: qsTr("History")
		}

		// The search field of the history.
		Rectangle {
			id: search
			anchors.top: historyLabel.bottom
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
			anchors { top: search.bottom; topMargin: sidebar.historyGap; bottom: footerBar.top }
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
