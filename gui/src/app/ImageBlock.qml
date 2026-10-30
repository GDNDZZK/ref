import QtQuick
import RefGui

// An image attached to a message. Only its name is shown for now.
Block {
	id: block

	Rectangle {
		width: Math.min(label.implicitWidth + 2 * 14, block.columnWidth)
		height: label.implicitHeight + 2 * 8
		radius: Theme.radius
		color: Theme.surface
		border.color: Theme.border

		Text {
			id: label
			x: 14
			y: 8
			width: parent.width - 2 * 14
			text: qsTr("Image: %1").arg(block.content)
			elide: Text.ElideRight
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}
}
