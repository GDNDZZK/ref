import QtQuick
import RefGui

// A shell command as typed: "$ command" in bold monospace on the code background.
Rectangle {
	id: box

	required property string command
	property bool copyable: false

	readonly property int verticalPadding: 8
	readonly property int horizontalPadding: 10
	readonly property int copyReserve: 28 // keeps the text clear of the copy button

	height: text.height + 2 * verticalPadding
	radius: Theme.radiusSmall
	color: Theme.codeBackground

	Text {
		textFormat: Text.PlainText
		id: text
		x: box.horizontalPadding
		y: box.verticalPadding
		width: parent.width - box.horizontalPadding - (box.copyable ? box.copyReserve : box.horizontalPadding)
		text: "$ " + box.command
		color: Theme.text
		font.family: Theme.monoFamily
		font.pixelSize: Theme.codeFontSize
		font.bold: true
		wrapMode: Text.Wrap
	}

	HoverCopyButton {
		anchors { top: parent.top; right: parent.right; margins: 4 }
		visible: box.copyable
		text: box.command
	}
}
