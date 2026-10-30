import QtQuick
import RefGui

// Key caps of a shortcut, e.g. ["Ctrl", "Shift", "N"], as small bordered chips.
Row {
	id: chips

	property var keys: []

	spacing: 4

	Repeater {
		model: chips.keys

		Rectangle {
			required property string modelData
			width: label.implicitWidth + 2 * 6
			height: label.implicitHeight + 2 * 2
			radius: Theme.radiusSmall
			color: "transparent"
			border.color: Theme.border

			Text {
				textFormat: Text.PlainText
				id: label
				anchors.centerIn: parent
				text: parent.modelData
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}
		}
	}
}
