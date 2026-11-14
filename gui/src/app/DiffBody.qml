import QtQuick
import QtQuick.Layouts
import RefGui

// An edit or a write: collapsed shows a diff preview (path, +/- counts, first rows);
// expanded shows the raw input, full diff, warnings and output.
ToolBodyBase {
	id: body

	property bool preview: false

	signal expandRequested()

	readonly property var meta: call.meta
	readonly property string diff: Fields.stringOf(meta, "diff")
	readonly property string path: Fields.stringOf(meta, "path")
	readonly property string input: call.inputJson !== "" ? call.inputJson : call.rawInput
	readonly property int previewLimit: 8
	readonly property int fullLimit: 200

	ToolSection {
		visible: !body.preview && body.input !== ""
		width: body.contentWidth
		label: qsTr("Input")

		OutputText {
			text: body.input
			textColor: Theme.infoText
		}
	}

	RowLayout {
		visible: body.path !== ""
		width: body.contentWidth
		spacing: 10

		Text {
			textFormat: Text.PlainText
			Layout.fillWidth: true
			Layout.maximumWidth: implicitWidth
			text: body.path
			elide: Text.ElideMiddle
			color: Theme.text
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			textFormat: Text.PlainText
			visible: (body.meta.added ?? 0) > 0
			text: "+" + body.meta.added
			color: Theme.diffAdded
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			textFormat: Text.PlainText
			visible: (body.meta.removed ?? 0) > 0
			text: "-" + body.meta.removed
			color: Theme.diffRemoved
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Item {
			Layout.fillWidth: true
		}
	}

	LineView {
		id: diffView
		width: body.contentWidth
		rows: ToolInfo.diffRows(body.diff)
		copyText: body.diff
		previewLines: body.preview ? body.previewLimit : body.fullLimit
		showAll: !body.preview && body.call.showAll
		moreText: body.preview ? qsTr("+%1 more").arg(diffView.hiddenRows) : qsTr("Show %n more line(s)", "", diffView.hiddenRows)
		onShowAllRequested: body.preview ? body.expandRequested() : body.showAllRequested()
	}
	Column {
		visible: body.call.warnings.length > 0
		width: body.contentWidth
		spacing: 4

		Repeater {
			model: body.call.warnings

			Text {
				required property string modelData
				textFormat: Text.PlainText
				width: parent.width
				text: modelData
				color: Theme.warningText
				font.pixelSize: Theme.smallFontSize
				wrapMode: Text.Wrap
			}
		}
	}

	OutputSection {
		visible: !body.preview && (body.call.output !== "" || body.call.toolState === ToolState.done || body.call.toolState === ToolState.error)
		call: body.call
		width: body.contentWidth
		onShowAllRequested: body.showAllRequested()
	}
}
