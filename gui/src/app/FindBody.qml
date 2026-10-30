pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// Found paths, one to a row; folders end with a slash.
ToolBodyBase {
	id: body

	readonly property var meta: call.meta
	readonly property int rowHeight: 24
	readonly property var paths: call.output.split("\n").filter(line => line !== "" && !line.startsWith("[Showing "))

	Column {
		width: body.contentWidth

		Repeater {
			model: body.paths

			delegate: Row {
				id: entry

				required property string modelData
				readonly property var parts: ToolInfo.splitPath(modelData)

				height: body.rowHeight
				spacing: 8

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: entry.parts.isDirectory ? "folder" : "file"
					color: Theme.textMuted
				}
				Row {
					anchors.verticalCenter: parent.verticalCenter

					Text {
						textFormat: Text.PlainText
						text: entry.parts.directory
						color: Theme.textMuted
						font.family: Theme.monoFamily
						font.pixelSize: Theme.smallFontSize
					}
					Text {
						textFormat: Text.PlainText
						text: entry.parts.name + (entry.parts.isDirectory ? "/" : "")
						color: Theme.text
						font.family: Theme.monoFamily
						font.pixelSize: Theme.smallFontSize
					}
				}
			}
		}
	}

	Row {
		visible: body.meta.count !== undefined
		spacing: 6

		ToolChip {
			text: qsTr("%n result(s)", "", body.meta.count ?? 0)
		}
		ToolChip {
			visible: body.meta.truncated === true
			text: qsTr("truncated")
		}
	}
}
