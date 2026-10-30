import QtQuick
import RefGui

// A call that talks to another session: who it concerns, what was said, and the answer.
ToolBodyBase {
	id: body

	readonly property var input: call.inputObject
	readonly property var icons: ({
		"send_message": "chat-send",
		"create_session": "chat-plus",
		"list_sessions": "chat-list",
		"message_status": "chat-send",
		"close_session": "chat-x",
		"read_session": "chat"
	})
	readonly property string who: ToolInfo.firstText(input, ["to", "name", "session"])
	readonly property string said: ToolInfo.firstText(input, ["message", "prompt"])
	readonly property int barWidth: 3
	readonly property int barGap: 10

	Row {
		visible: body.who !== ""
		spacing: 8

		Icon {
			anchors.verticalCenter: parent.verticalCenter
			name: body.icons[body.call.toolName] ?? "chat"
			color: Theme.info
		}
		Text {
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			text: body.who
			color: Theme.info
			font.pixelSize: Theme.codeFontSize
			font.bold: true
		}
	}

	Item {
		visible: body.said !== ""
		width: body.contentWidth
		height: said.height

		Rectangle {
			width: body.barWidth
			height: parent.height
			radius: width / 2
			color: Theme.info
		}
		ProseText {
			id: said
			x: body.barWidth + body.barGap
			width: parent.width - x
			text: body.said
			textFormat: TextEdit.PlainText
			font.pixelSize: Theme.codeFontSize
		}
	}

	OutputSection {
		call: body.call
		width: body.contentWidth
		onShowAllRequested: body.showAllRequested()
	}
}
