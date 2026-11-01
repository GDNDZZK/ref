import QtQuick
import RefGui

// Says that the output of a tool call was cut short. `meta` is the call's meta.
ToolChip {
	property var meta

	visible: meta.truncated === true
	text: qsTr("truncated")
}
