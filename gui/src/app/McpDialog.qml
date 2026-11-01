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

			McpServerRow {}
		}

		Text {
			textFormat: Text.PlainText
			width: parent.width
			height: Theme.emptyStateHeight
			horizontalAlignment: Text.AlignHCenter
			verticalAlignment: Text.AlignVCenter
			visible: Workspace.mcpServers.length === 0
			text: qsTr("No MCP servers configured")
			color: Theme.textMuted
			font.pixelSize: Theme.secondaryFontSize
		}
	}
}
