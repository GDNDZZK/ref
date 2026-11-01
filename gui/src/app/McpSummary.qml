import QtQuick
import RefGui

// The MCP servers in the sidebar footer: a dot per server (the first few), and the count of the rest. A click opens the dialog.
Row {
	id: summary

	readonly property int maxDots: 6

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
		font.pixelSize: Theme.captionFontSize
	}
	Row {
		anchors.verticalCenter: parent.verticalCenter
		spacing: 4

		Repeater {
			model: Workspace.mcpServers.slice(0, summary.maxDots)

			Rectangle {
				id: dot
				required property var modelData
				anchors.verticalCenter: parent.verticalCenter
				width: 6
				height: 6
				radius: 3
				color: McpStatus.colorOf(modelData.status)

				Pulse {
					target: dot
					running: dot.modelData.status === McpStatus.connecting
				}
			}
		}
	}
	Text {
		textFormat: Text.PlainText
		anchors.verticalCenter: parent.verticalCenter
		visible: Workspace.mcpServers.length > summary.maxDots
		text: "+" + (Workspace.mcpServers.length - summary.maxDots)
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}

	HoverHandler { cursorShape: Qt.PointingHandCursor }
	TapHandler { onTapped: Commands.run(CommandId.mcp) }
}
