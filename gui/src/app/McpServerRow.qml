import QtQuick
import RefGui

// One MCP server in the list: its status, and when it has an error or tools, a fold-out with them.
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
		x: Theme.dialogPadding
		y: (Theme.sidebarHistoryRowHeight - height) / 2
		width: Theme.statusDotSize
		height: Theme.statusDotSize
		radius: width / 2
		color: McpStatus.colorOf(server.modelData.status)
	}
	Pulse {
		target: dot
		running: server.modelData.status === McpStatus.connecting
	}

	Text {
		textFormat: Text.PlainText
		anchors { left: parent.left; leftMargin: Theme.dialogPadding + 20; right: status.left; rightMargin: 12 }
		height: Theme.sidebarHistoryRowHeight
		verticalAlignment: Text.AlignVCenter
		text: server.modelData.name
		color: Theme.text
		font.pixelSize: Theme.fontSize
		elide: Text.ElideRight
	}
	Text {
		textFormat: Text.PlainText
		id: status
		anchors { right: chevron.left; rightMargin: 8 }
		height: Theme.sidebarHistoryRowHeight
		verticalAlignment: Text.AlignVCenter
		text: server.modelData.status
		color: server.modelData.status === McpStatus.failed ? Theme.dangerText : Theme.textMuted
		font.pixelSize: Theme.captionFontSize
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
		x: Theme.dialogPadding + 20
		y: Theme.sidebarHistoryRowHeight
		width: parent.width - (Theme.dialogPadding + 20) - Theme.dialogPadding
		spacing: 8

		Text {
			textFormat: Text.PlainText
			width: parent.width
			visible: server.modelData.error !== ""
			text: server.modelData.error
			color: Theme.dangerText
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
						textFormat: Text.PlainText
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
