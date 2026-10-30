import QtQuick
import RefGui

// A part of a tool card's body under a small label.
Column {
	id: section

	property alias label: caption.text
	default property alias content: holder.data

	spacing: 6

	Text {
		id: caption
		visible: text !== ""
		color: Theme.textMuted
		font.pixelSize: 11
		font.weight: Font.Medium
	}

	Item {
		id: holder
		width: parent.width
		height: childrenRect.height
	}
}
