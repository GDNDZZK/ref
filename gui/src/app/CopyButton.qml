import QtQuick
import RefGui

// Copies `text` to the clipboard. The icon turns into a tick for a moment.
Item {
	id: button

	required property string text

	readonly property int hitSize: 24
	readonly property bool copied: flag.copied

	implicitWidth: hitSize
	implicitHeight: hitSize

	CopiedFlag { id: flag }

	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: hover.hovered ? Theme.surfaceHover : "transparent"
	}

	Icon {
		anchors.centerIn: parent
		name: button.copied ? "check" : "copy"
		color: button.copied ? Theme.successText : Theme.textMuted
	}

	HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
	TapHandler {
		onTapped: {
			Clipboard.setText(button.text)
			flag.flash()
		}
	}
	Accessible.role: Accessible.Button
	Accessible.name: qsTr("Copy")
}
