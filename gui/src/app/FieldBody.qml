import QtQuick
import RefGui

// The body of a dialog that is one input field: the field sits inside the dialog padding.
Item {
	default property alias field: slot.data

	width: parent.width
	implicitHeight: slot.childrenRect.height + 2 * Theme.dialogPadding
	height: implicitHeight

	Item {
		id: slot
		x: Theme.dialogPadding
		y: Theme.dialogPadding
		width: parent.width - 2 * Theme.dialogPadding
	}
}
