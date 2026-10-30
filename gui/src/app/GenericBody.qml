import QtQuick
import RefGui

// The input and the output of any tool, as text.
ToolBodyBase {
	id: body

	readonly property string input: call.inputJson !== "" ? call.inputJson : call.rawInput

	ToolSection {
		visible: body.input !== ""
		width: body.contentWidth
		label: qsTr("Input")

		OutputText {
			text: body.input
			textColor: Theme.info
		}
	}

	OutputSection {
		call: body.call
		width: body.contentWidth
		onShowAllRequested: body.showAllRequested()
	}
}
