import QtQuick
import RefGui

// An edit or a write: the file, how many lines changed, and the diff.
ToolBodyBase {
	id: body

	readonly property var meta: call.meta
	readonly property string diff: typeof meta.diff === "string" ? meta.diff : ""
	readonly property string path: typeof meta.path === "string" ? meta.path : ""

	Row {
		visible: body.path !== ""
		spacing: 10

		Text {
			text: body.path
			color: Theme.text
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			visible: (body.meta.added ?? 0) > 0
			text: "+" + body.meta.added
			color: Theme.diffAdded
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			visible: (body.meta.removed ?? 0) > 0
			text: "-" + body.meta.removed
			color: Theme.diffRemoved
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
	}

	LineView {
		width: body.contentWidth
		rows: ToolInfo.diffRows(body.diff)
		copyText: body.diff
		showAll: body.call.showAll
		onShowAllRequested: body.showAllRequested()
	}

	OutputSection {
		visible: body.call.failed
		call: body.call
		width: body.contentWidth
	}
}
