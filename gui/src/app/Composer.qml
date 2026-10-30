// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import QtQuick.Controls.Basic
import RefGui

// Message input. Enter sends, Shift+Enter inserts a line break.
// While a reply is running, the send button turns into stop.
Rectangle {
	id: composer

	property bool busy: false
	signal submitted(string text)
	signal stopRequested()

	readonly property int padding: 8
	readonly property int maxInputHeight: 220
	readonly property real sendButtonCenterX: sendButton.x + sendButton.width / 2 // to line up controls above it

	function focusInput() {
		input.forceActiveFocus()
	}

	function submit() {
		const blank = input.text.trim() === ""
		if (busy || blank)
			return
		submitted(input.text)
		input.clear()
	}

	implicitHeight: Math.min(input.implicitHeight, maxInputHeight) + 2 * padding
	radius: Theme.radiusLarge
	color: Theme.surface
	border.color: input.activeFocus ? Theme.accent : Theme.border

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	ScrollView {
		anchors {
			left: parent.left
			right: sendButton.left
			top: parent.top
			bottom: parent.bottom
			margins: composer.padding
			leftMargin: composer.padding + 4
		}

		TextArea {
			id: input
			placeholderText: qsTr("Message ref")
			placeholderTextColor: Theme.textMuted
			wrapMode: TextArea.Wrap
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.fontSize
			background: null

			Keys.onPressed: event => {
				const isEnter = event.key === Qt.Key_Return || event.key === Qt.Key_Enter
				const wantsLineBreak = (event.modifiers & Qt.ShiftModifier) !== 0
				if (!isEnter || wantsLineBreak || input.inputMethodComposing)
					return
				event.accepted = true
				composer.submit()
			}
		}
	}

	SendButton {
		id: sendButton
		anchors { right: parent.right; bottom: parent.bottom; margins: composer.padding }
		busy: composer.busy
		canSend: input.text.trim() !== ""
		onClicked: composer.busy ? composer.stopRequested() : composer.submit()
	}
}
