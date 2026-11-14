pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// Answered questions and chosen answers of a `question` call. Collapsed shows the compact
// summary; expanded adds the raw input and output.
ToolBodyBase {
	id: body

	property bool preview: false

	readonly property string input: call.inputJson !== "" ? call.inputJson : call.rawInput
	readonly property var summary: ToolInfo.questionSummary(call.inputObject, call.meta)
	readonly property int rowSpacing: 6
	readonly property int entrySpacing: 8
	readonly property real maxHeaderShare: 0.45

	Column {
		width: body.contentWidth
		spacing: body.rowSpacing

		Repeater {
			model: body.summary

			Row {
				id: entry

				required property var modelData

				width: parent.width
				spacing: body.entrySpacing

				Text {
					id: entryHeader
					textFormat: Text.PlainText
					width: Math.min(implicitWidth, entry.width * body.maxHeaderShare)
					text: entry.modelData.header
					elide: Text.ElideRight
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
				Text {
					textFormat: Text.PlainText
					width: Math.max(0, entry.width - entryHeader.width - entry.spacing)
					text: entry.modelData.answered ? entry.modelData.answer : qsTr("Unanswered")
					color: entry.modelData.answered ? Theme.text : Theme.warningText
					font.pixelSize: Theme.smallFontSize
					wrapMode: Text.Wrap
				}
			}
		}
	}

	Column {
		visible: !body.preview
		width: body.contentWidth
		spacing: body.sectionSpacing

		ToolSection {
			visible: body.input !== ""
			width: parent.width
			label: qsTr("Input")

			OutputText {
				text: body.input
				textColor: Theme.infoText
			}
		}

		OutputSection {
			call: body.call
			width: parent.width
			onShowAllRequested: body.showAllRequested()
		}
	}
}
