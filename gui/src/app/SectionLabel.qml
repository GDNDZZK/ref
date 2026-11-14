import QtQuick
import RefGui

// The muted heading above a group of rows. `leftPadding` is the inset of the list it sits in.
Text {
	textFormat: Text.PlainText
	width: parent.width
	height: Theme.sidebarSectionHeight
	leftPadding: Theme.dialogPadding
	verticalAlignment: Text.AlignVCenter
	color: Theme.textMuted
	font.pixelSize: Theme.smallFontSize
}
