import QtQuick
import RefGui

// The frame of what a tool card shows when it is open: padding and the space between
// sections. `call` is the ToolBlock; the bodies read what they show from it.
Column {
	id: body

	required property var call

	signal showAllRequested()

	readonly property int horizontalPadding: 14
	readonly property int verticalPadding: 10
	readonly property int sectionSpacing: 12
	readonly property real contentWidth: width - 2 * horizontalPadding
	readonly property bool finished: call.toolState === "done" || call.toolState === "error" || call.toolState === "denied" || call.toolState === "cancelled"

	topPadding: verticalPadding
	bottomPadding: verticalPadding + 2
	leftPadding: horizontalPadding
	rightPadding: horizontalPadding
	spacing: sectionSpacing
}
