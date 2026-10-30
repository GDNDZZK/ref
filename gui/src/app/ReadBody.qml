import QtQuick
import RefGui

// A file read: its path and tag, and the lines with their numbers.
ToolBodyBase {
	id: body

	readonly property var groups: ToolInfo.listing(call.output)
	readonly property var group: groups.length > 0 ? groups[0] : null

	Row {
		visible: body.group !== null && body.group.path !== ""
		spacing: 8

		Text {
			textFormat: Text.PlainText
			text: body.group ? body.group.path : ""
			color: Theme.text
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			textFormat: Text.PlainText
			text: body.group ? "#" + body.group.tag : ""
			color: Theme.textMuted
			opacity: 0.7
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
	}

	LineView {
		width: body.contentWidth
		rows: body.group ? body.group.rows : []
		copyText: body.group ? body.group.rows.map(row => row.text).join("\n") : ""
		showAll: body.call.showAll
		onShowAllRequested: body.showAllRequested()
	}

	Text {
		textFormat: Text.PlainText
		visible: body.group !== null && body.group.notes.length > 0
		width: body.contentWidth
		text: body.group ? body.group.notes.join("\n") : ""
		wrapMode: Text.Wrap
		color: Theme.textMuted
		font.family: Theme.monoFamily
		font.pixelSize: Theme.smallFontSize
	}
}
