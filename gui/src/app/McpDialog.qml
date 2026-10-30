import QtQuick
import RefGui

// Status of the MCP servers; a row with an error or tools opens to show them.
ModalDialog {
	id: dialog

	title: qsTr("MCP servers")
	buttons: [
		DialogButton {
			text: qsTr("Refresh")
			onClicked: Workspace.refreshMcp()
		}
	]

	Column {
		width: parent.width

		Repeater {
			model: Workspace.mcpServers

			Item {
				id: server

				required property var modelData
				property bool expanded: false
				readonly property bool hasDetails: modelData.error !== "" || modelData.tools.length > 0

				width: parent.width
				height: Theme.sidebarHistoryRowHeight + (expanded ? details.implicitHeight + 12 : 0)
				clip: true

				Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

				Rectangle {
					id: dot
					x: 22
					y: (Theme.sidebarHistoryRowHeight - height) / 2
					width: 8
					height: 8
					radius: 4
					color: server.modelData.status === "connected" ? Theme.success
						: server.modelData.status === "failed" ? Theme.danger
						: server.modelData.status === "connecting" ? Theme.accent
						: Qt.alpha(Theme.textMuted, 0.5)
				}
				Pulse {
					target: dot
					running: server.modelData.status === "connecting"
				}

				Text {
					anchors { left: parent.left; leftMargin: 42; right: status.left; rightMargin: 12 }
					height: Theme.sidebarHistoryRowHeight
					verticalAlignment: Text.AlignVCenter
					text: server.modelData.name
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}
				Text {
					id: status
					anchors { right: chevron.left; rightMargin: 8 }
					height: Theme.sidebarHistoryRowHeight
					verticalAlignment: Text.AlignVCenter
					text: server.modelData.status
					color: server.modelData.status === "failed" ? Theme.danger : Theme.textMuted
					font.pixelSize: Theme.smallFontSize + 1
				}
				Chevron {
					id: chevron
					anchors { right: parent.right; rightMargin: 18 }
					y: (Theme.sidebarHistoryRowHeight - height) / 2
					visible: server.hasDetails
					rotation: server.expanded ? 90 : 0

					Behavior on rotation { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}

				TapHandler {
					enabled: server.hasDetails
					onTapped: server.expanded = !server.expanded
				}
				HoverHandler {
					enabled: server.hasDetails
					cursorShape: Qt.PointingHandCursor
				}

				Column {
					id: details
					x: 42
					y: Theme.sidebarHistoryRowHeight
					width: parent.width - 42 - 22
					spacing: 8

					Text {
						width: parent.width
						visible: server.modelData.error !== ""
						text: server.modelData.error
						color: Theme.danger
						font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						wrapMode: Text.Wrap
					}
					Flow {
						width: parent.width
						spacing: 6

						Repeater {
							model: server.modelData.tools

							Rectangle {
								required property string modelData
								width: label.implicitWidth + 2 * 6
								height: label.implicitHeight + 2 * 2
								radius: Theme.radiusSmall
								color: Theme.codeBackground

								Text {
									id: label
									anchors.centerIn: parent
									text: parent.modelData
									color: Theme.textMuted
									font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
								}
							}
						}
					}
				}
			}
		}

		Text {
			width: parent.width
			height: 64
			horizontalAlignment: Text.AlignHCenter
			verticalAlignment: Text.AlignVCenter
			visible: Workspace.mcpServers.length === 0
			text: qsTr("No MCP servers configured")
			color: Theme.textMuted
			font.pixelSize: Theme.fontSize - 1
		}
	}
}
