import QtQuick
import Qutie

// Read-only, selectable text in the conversation's body style.
TextEdit {
	id: prose

	readOnly: true
	selectByMouse: true
	wrapMode: TextEdit.Wrap
	color: Theme.text
	selectionColor: Theme.selection
	selectedTextColor: Theme.text
	font.pixelSize: Theme.fontSize

	onLinkActivated: link => Qt.openUrlExternally(link)

	HoverHandler {
		cursorShape: prose.hoveredLink ? Qt.PointingHandCursor : Qt.IBeamCursor
	}
}
