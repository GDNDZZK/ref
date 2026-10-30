pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// The sessions of the window: those running, the past ones with a search field, and the MCP
// status. The content has a fixed width; Main animates the width of the sidebar itself, so
// the content slides out to the left, clipped.
Rectangle {
	id: sidebar

	readonly property int contentWidth: Theme.sidebarWidth
	readonly property int maxMcpDots: 6
	// Space for the two lists: what is left of the column after the fixed parts.
	readonly property real listSpace: height - Theme.sidebarHeaderHeight - 44 - 2 * Theme.sidebarSectionHeight - 32 - 8 - Theme.sidebarFooterHeight

	color: Theme.surface
	clip: true

	signal collapseRequested()

	Rectangle {
		anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
		width: 1
		color: Theme.border
	}

	Item {
		id: column
		anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
		width: sidebar.contentWidth

		Item {
			id: header
			width: parent.width
			height: Theme.sidebarHeaderHeight

			Text {
				textFormat: Text.PlainText
				x: 18
				anchors.verticalCenter: parent.verticalCenter
				text: "ref"
				color: Theme.text
				font { pixelSize: Theme.fontSize + 1; bold: true }
			}
			CircleButton {
				anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
				implicitWidth: 32
				implicitHeight: 32
				Accessible.name: qsTr("Hide sidebar")
				onClicked: sidebar.collapseRequested()

				Icon {
					anchors.centerIn: parent
					name: "sidebar"
					size: 20
					color: Theme.textMuted
				}
			}
		}

		Item {
			id: newRow
			anchors.top: header.bottom
			width: parent.width
			height: 44

			Rectangle {
				id: newButton
				x: Theme.sidebarPadding
				anchors.verticalCenter: parent.verticalCenter
				width: parent.width - 2 * Theme.sidebarPadding - folderButton.width - 8
				height: 36
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
						font.pixelSize: Theme.smallFontSize + 1
					}
				}
			}
			HoverHandler {
				id: newHover
				cursorShape: Qt.PointingHandCursor
			}
			TapHandler {
				id: newTap
				onTapped: Commands.run("new")
			}

			CircleButton {
				id: folderButton
				anchors { right: parent.right; rightMargin: Theme.sidebarPadding; verticalCenter: parent.verticalCenter }
				implicitWidth: 36
				implicitHeight: 36
				Accessible.name: qsTr("New session in another folder…")
				onClicked: Commands.run("new-in")

				Icon {
					anchors.centerIn: parent
					name: "folder-open"
					size: 20
					color: Theme.textMuted
				}
			}
		}

		SectionLabel {
			id: runningLabel
			anchors.top: newRow.bottom
			text: qsTr("Running")
		}

		ListView {
			id: running
			anchors.top: runningLabel.bottom
			width: parent.width
			height: Math.min(contentHeight, Math.max(sidebar.listSpace / 2, Theme.sidebarMinRunningRows * (Theme.sidebarRowHeight + 2)))
			model: Workspace.sessions
			spacing: 2
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
			displaced: Transition {
				NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
			}

			ScrollBar.vertical: ThemedScrollBar {}
		}

		SectionLabel {
			id: historyLabel
			anchors.top: running.bottom
			text: qsTr("History")
		}

		// The search field of the history.
		Rectangle {
			id: search
			anchors.top: historyLabel.bottom
			x: Theme.sidebarPadding
			width: parent.width - 2 * Theme.sidebarPadding
			height: 32
			radius: 16
			color: Theme.surface
			border.color: searchInput.activeFocus ? Theme.accent : Theme.border

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
				font.pixelSize: Theme.smallFontSize + 1
				clip: true
				Accessible.name: qsTr("Search history")
				onTextChanged: Workspace.history.filter = text.trim()

				Text {
					textFormat: Text.PlainText
					anchors.fill: parent
					verticalAlignment: Text.AlignVCenter
					text: qsTr("Search history")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize + 1
					visible: searchInput.text === "" && searchInput.preeditText === ""
				}
			}
			CircleButton {
				id: clear
				anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
				implicitWidth: 22
				implicitHeight: 22
				opacity: searchInput.text !== "" ? 1 : 0
				visible: opacity > 0
				Accessible.name: qsTr("Clear search")
				onClicked: searchInput.text = ""

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Icon {
					anchors.centerIn: parent
					name: "close"
					size: 16
					color: Theme.textMuted
				}
			}
		}

		ListView {
			id: history
			anchors { top: search.bottom; topMargin: 8; bottom: footerBar.top }
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
				text: !Workspace.historyLoaded ? qsTr("Loading…") : searchInput.text !== "" ? qsTr("No matching sessions") : qsTr("No earlier sessions")
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize + 1
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

			Row {
				id: mcp
				x: 18
				anchors.verticalCenter: parent.verticalCenter
				spacing: 8
				visible: Workspace.mcpServers.length > 0

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: "mcp"
					size: 16
					color: Theme.textMuted
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					text: "MCP"
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize + 1
				}
				Row {
					anchors.verticalCenter: parent.verticalCenter
					spacing: 4

					Repeater {
						model: Workspace.mcpServers.slice(0, sidebar.maxMcpDots)

						Rectangle {
							id: dot
							required property var modelData
							anchors.verticalCenter: parent.verticalCenter
							width: 6
							height: 6
							radius: 3
							color: modelData.status === "connected" ? Theme.success
								: modelData.status === "failed" ? Theme.danger
								: modelData.status === "connecting" ? Theme.accent
								: Qt.alpha(Theme.textMuted, 0.5)

							Pulse {
								target: dot
								running: dot.modelData.status === "connecting"
							}
						}
					}
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					visible: Workspace.mcpServers.length > sidebar.maxMcpDots
					text: "+" + (Workspace.mcpServers.length - sidebar.maxMcpDots)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: Commands.run("mcp") }
			}

			CircleButton {
				anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
				implicitWidth: 32
				implicitHeight: 32
				Accessible.name: qsTr("Settings")
				onClicked: Commands.run("settings")

				Icon {
					anchors.centerIn: parent
					name: "sliders"
					size: 20
					color: Theme.textMuted
				}
			}
		}
	}

	component SectionLabel: Text {

		textFormat: Text.PlainText
		width: column.width
		height: Theme.sidebarSectionHeight
		leftPadding: 18
		verticalAlignment: Text.AlignVCenter
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}
}
