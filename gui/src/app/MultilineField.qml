import QtQuick
import QtQuick.Controls
import RefGui

// Multi-line plain-text editor with bounded auto-grow height and vertical scrolling.
FocusScope {
	id: field

	property alias text: editor.text
	property string placeholderText: ""
	property string errorText: ""
	property bool monospace: false
	property bool readOnly: false
	property int minHeight: 96
	property int maxHeight: 260

	signal edited()

	readonly property bool hasError: errorText !== ""
	readonly property real boxHeight: Math.min(maxHeight, Math.max(minHeight, editor.contentHeight + 20))

	function keepCursorVisible() {
		const top = editor.cursorRectangle.y
		const bottom = top + editor.cursorRectangle.height
		if (top < scroller.contentY)
			scroller.contentY = top
		else if (bottom > scroller.contentY + scroller.height)
			scroller.contentY = bottom - scroller.height
	}

	implicitWidth: 360
	implicitHeight: boxHeight + (hasError ? errorLabel.implicitHeight + 6 : 0)
	opacity: enabled ? 1 : Theme.disabledOpacity

	Rectangle {
		id: box
		width: parent.width
		height: field.boxHeight
		radius: Theme.radius
		color: field.monospace ? Theme.codeBackground : Theme.background
		border.color: field.hasError ? Theme.dangerText : (editor.activeFocus ? Theme.accentText : Theme.border)
		clip: true

		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Flickable {
			id: scroller
			anchors { fill: parent; leftMargin: 12; rightMargin: 12; topMargin: 10; bottomMargin: 10 }
			contentWidth: width
			contentHeight: Math.max(height, editor.contentHeight)
			boundsBehavior: Flickable.StopAtBounds
			clip: true

			TextEdit {
				id: editor
				textFormat: TextEdit.PlainText
				width: scroller.width
				height: Math.max(scroller.height, contentHeight)
				readOnly: field.readOnly
				selectByMouse: true
				wrapMode: TextEdit.Wrap
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font {
					family: field.monospace ? Theme.monoFamily : ""
					pixelSize: field.monospace ? Theme.codeFontSize : Theme.secondaryFontSize
				}
				focus: true
				Accessible.name: field.placeholderText
				onCursorRectangleChanged: field.keepCursorVisible()
				onTextChanged: {
					if (activeFocus)
						field.edited()
				}
			}

			Text {
				textFormat: Text.PlainText
				visible: editor.length === 0 && field.placeholderText !== ""
				width: scroller.width
				text: field.placeholderText
				color: Theme.textMuted
				font {
					family: field.monospace ? Theme.monoFamily : ""
					pixelSize: field.monospace ? Theme.codeFontSize : Theme.secondaryFontSize
				}
				wrapMode: Text.Wrap
			}

			ScrollBar.vertical: ThemedScrollBar {}
		}
	}

	Text {
		id: errorLabel
		textFormat: Text.PlainText
		visible: field.hasError
		anchors { top: box.bottom; topMargin: 6; left: parent.left; right: parent.right }
		text: field.errorText
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}
}
