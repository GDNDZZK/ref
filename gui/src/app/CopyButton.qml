import QtQuick
import RefGui

// Copies `text` to the clipboard. The icon turns into a tick for a moment.
Item {
	id: button

	required property string text

	readonly property int hitSize: 24
	property bool copied: false

	implicitWidth: hitSize
	implicitHeight: hitSize

	Timer {
		id: reset
		interval: Motion.slow * 3
		onTriggered: button.copied = false
	}

	// QML has no clipboard of its own; a TextEdit copies its selection.
	TextEdit {
		id: clipboard
		visible: false
	}

	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: hover.hovered ? Theme.surfaceHover : "transparent"
	}

	Icon {
		anchors.centerIn: parent
		name: button.copied ? "check" : "copy"
		color: button.copied ? Theme.success : Theme.textMuted
	}

	HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
	TapHandler {
		onTapped: {
			clipboard.text = button.text
			clipboard.selectAll()
			clipboard.copy()
			button.copied = true
			reset.restart()
		}
	}
	Accessible.role: Accessible.Button
	Accessible.name: qsTr("Copy")
}
