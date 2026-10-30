import QtQuick
import RefGui

// Single-line text field of dialogs, with an optional error line under it that fades in.
Item {
	id: field

	property alias text: input.text
	property string placeholderText: ""
	property string errorText: ""
	property alias inputItem: input
	readonly property bool hasError: errorText !== ""

	signal accepted()
	signal edited()
	signal tabPressed()
	signal upPressed()
	signal downPressed()

	function selectAll() {
		input.selectAll()
	}

	implicitWidth: 240
	implicitHeight: Theme.fieldHeight + (hasError ? errorLabel.implicitHeight + 6 : 0)

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Rectangle {
		id: box
		width: parent.width
		height: Theme.fieldHeight
		radius: 10
		color: Theme.background
		border.color: field.hasError ? Theme.danger : input.activeFocus ? Theme.accent : Theme.border

		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		TextInput {
			id: input
			anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
			verticalAlignment: TextInput.AlignVCenter
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.fontSize
			clip: true
			focus: true
			onAccepted: field.accepted()
			onTextEdited: field.edited()
			Keys.onTabPressed: event => { field.tabPressed(); event.accepted = true }
			Keys.onUpPressed: event => { field.upPressed(); event.accepted = true }
			Keys.onDownPressed: event => { field.downPressed(); event.accepted = true }
			Accessible.name: field.placeholderText
		}

		Text {
			anchors { fill: input }
			verticalAlignment: Text.AlignVCenter
			text: field.placeholderText
			color: Theme.textMuted
			font.pixelSize: Theme.fontSize
			elide: Text.ElideRight
			visible: input.text === "" && input.preeditText === ""
		}
	}

	Text {
		id: errorLabel
		anchors { top: box.bottom; topMargin: 6; left: parent.left; right: parent.right }
		text: field.errorText
		color: Theme.danger
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
		opacity: field.hasError ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}
}
