pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// Search results: for each file its path and the matching lines with their context.
ToolBodyBase {
	id: body

	readonly property var groups: ToolInfo.listing(call.output)
	readonly property var meta: call.meta
	// The counts come with the finished result; a running call or one cut short has none.
	readonly property bool hasCounts: Fields.hasNumber(meta, "matches") && Fields.hasNumber(meta, "files")
	readonly property int matchCount: hasCounts ? meta.matches : 0 // only shown when hasCounts
	readonly property int fileCount: hasCounts ? meta.files : 0

	Repeater {
		model: body.groups

		delegate: Column {
			id: group

			required property var modelData

			width: body.contentWidth
			spacing: 4

			Text {
				textFormat: Text.PlainText
				visible: group.modelData.path !== ""
				width: parent.width
				text: group.modelData.path
				elide: Text.ElideMiddle
				color: Theme.text
				font.family: Theme.monoFamily
				font.pixelSize: Theme.smallFontSize
				font.bold: true
			}
			LineView {
				width: parent.width
				rows: group.modelData.rows
				showAll: body.call.showAll
				onShowAllRequested: body.showAllRequested()
			}
			Text {
				textFormat: Text.PlainText
				visible: group.modelData.notes.length > 0
				width: parent.width
				text: group.modelData.notes.join("\n")
				wrapMode: Text.Wrap
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}
		}
	}

	Row {
		visible: body.hasCounts || body.meta.truncated === true
		spacing: 6

		ToolChip {
			visible: body.hasCounts
			text: qsTr("%n match(es) in %1", "", body.matchCount).arg(qsTr("%n file(s)", "", body.fileCount))
		}
		TruncatedChip {
			meta: body.meta
		}
	}
}
