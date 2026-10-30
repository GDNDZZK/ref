import QtQuick
import RefGui

// What a tool card shows when it is open: its input and output, laid out by the tool.
Column {
	id: body

	required property string toolName
	required property string toolState
	required property string inputJson
	required property string rawInput
	required property string output
	required property bool failed
	required property bool showAll

	signal showAllRequested()

	readonly property int sectionSpacing: 12
	readonly property int verticalPadding: 10
	readonly property int horizontalPadding: 14
	readonly property string input: inputJson !== "" ? inputJson : rawInput
	readonly property bool finished: toolState === "done" || toolState === "error" || toolState === "denied" || toolState === "cancelled"

	topPadding: verticalPadding
	bottomPadding: verticalPadding + 2
	leftPadding: horizontalPadding
	rightPadding: horizontalPadding
	spacing: sectionSpacing

	ToolSection {
		visible: body.input !== ""
		width: body.width - 2 * body.horizontalPadding
		label: qsTr("Input")

		OutputText {
			text: body.input
			textColor: Theme.info
		}
	}

	ToolSection {
		visible: body.output !== "" || body.finished
		width: body.width - 2 * body.horizontalPadding
		label: qsTr("Output")

		OutputText {
			text: body.output !== "" ? body.output : qsTr("(no output)")
			textColor: body.failed ? Theme.danger : Theme.textMuted
			copyable: body.output !== ""
			showAll: body.showAll
			onShowAllRequested: body.showAllRequested()
		}
	}
}
