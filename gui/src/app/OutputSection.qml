import QtQuick
import RefGui

// The "Output" of a tool call as text: danger-colored when the call failed, a note when
// it finished without any.
ToolSection {
	id: section

	required property var call

	signal showAllRequested()

	visible: call.output !== "" || call.toolState === "done" || call.toolState === "error"
	label: qsTr("Output")

	OutputText {
		text: section.call.output !== "" ? section.call.output : qsTr("(no output)")
		textColor: section.call.failed || section.call.toolState === "error" ? Theme.danger : Theme.textMuted
		copyable: section.call.output !== ""
		showAll: section.call.showAll
		onShowAllRequested: section.showAllRequested()
	}
}
