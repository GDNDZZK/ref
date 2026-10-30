import QtQuick
import RefGui

// An action in a message's meta line: an icon and its name.
Item {
	id: action

	required property string iconName
	required property string label

	signal triggered

	implicitWidth: content.width + 2 * padding
	implicitHeight: 24
	anchors.verticalCenter: parent ? parent.verticalCenter : undefined

	readonly property int padding: 4

	Row {
		id: content
		anchors.centerIn: parent
		spacing: 4

		Icon {
			anchors.verticalCenter: parent.verticalCenter
			name: action.iconName
			size: 16
			color: hover.hovered ? Theme.text : Theme.textMuted
		}
		Text {
			anchors.verticalCenter: parent.verticalCenter
			text: action.label
			color: hover.hovered ? Theme.text : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}

	HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
	TapHandler { onTapped: action.triggered() }
	Accessible.role: Accessible.Button
	Accessible.name: action.label
}
