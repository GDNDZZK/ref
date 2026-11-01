import QtQuick
import RefGui

// A round button with one centered glyph. Set Accessible.name on each use.
CircleButton {
	id: iconButton

	property string iconName: ""
	property int iconSize: 20
	property color iconColor: Theme.textMuted
	property int buttonSize: 34

	implicitWidth: buttonSize
	implicitHeight: buttonSize

	Icon {
		anchors.centerIn: parent
		name: iconButton.iconName
		size: iconButton.iconSize
		color: iconButton.iconColor
	}
}
