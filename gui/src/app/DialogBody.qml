import QtQuick
import RefGui

// The body of a dialog: a column inside the dialog padding, with room above and below.
Column {
	x: Theme.dialogPadding
	width: parent.width - 2 * Theme.dialogPadding
	topPadding: 4
	bottomPadding: Theme.dialogPadding
}
