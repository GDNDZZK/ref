import QtQuick
import RefGui

// Selectable monospace text of a tool, cut after a number of lines until the user asks for
// all of it. A copy button appears over the top right corner while the pointer is over it.
Item {
	id: output

	required property string text
	property color textColor: Theme.textMuted
	property bool showAll: false
	property bool copyable: true

	signal showAllRequested()

	readonly property int previewLines: 200
	readonly property int lineHeight: 18
	readonly property var lines: text.split("\n")
	readonly property int hiddenLines: showAll ? 0 : Math.max(0, lines.length - previewLines)
	readonly property string shownText: hiddenLines > 0 ? lines.slice(0, previewLines).join("\n") : text

	width: parent.width
	height: body.height + (hiddenLines > 0 ? more.height : 0)

	TextEdit {
		id: body
		width: parent.width
		readOnly: true
		selectByMouse: true
		wrapMode: TextEdit.Wrap
		textFormat: TextEdit.PlainText
		text: output.shownText
		color: output.textColor
		selectionColor: Theme.selection
		selectedTextColor: Theme.text
		font.family: Theme.monoFamily
		font.pixelSize: Theme.smallFontSize
	}

	TextButton {
		id: more
		y: body.height
		visible: output.hiddenLines > 0
		text: qsTr("Show %n more line(s)", "", output.hiddenLines)
		onClicked: output.showAllRequested()
	}

	HoverHandler { id: hover }

	CopyButton {
		anchors { top: parent.top; right: parent.right }
		visible: output.copyable && output.text !== ""
		text: output.text
		opacity: hover.hovered ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
}
