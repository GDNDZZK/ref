// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Read-only, selectable text in the conversation's body style.
TextEdit {
	id: prose

	readOnly: true
	textFormat: TextEdit.PlainText // Markdown items say so; text from outside is never read as markup
	selectByMouse: true
	wrapMode: TextEdit.Wrap
	color: Theme.text
	selectionColor: Theme.selection
	selectedTextColor: Theme.text
	font.pixelSize: Theme.fontSize

	onLinkActivated: link => Links.open(link)

	HoverHandler {
		cursorShape: prose.hoveredLink ? Qt.PointingHandCursor : Qt.IBeamCursor
	}
}
