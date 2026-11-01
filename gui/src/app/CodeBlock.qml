// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui
import org.kde.syntaxhighlighting

// A fenced code block: language label, copy action and highlighted source.
Block {
	id: block

	required property string language

	Rectangle {
		width: block.columnWidth
		height: header.height + source.height
		radius: Theme.radius
		color: Theme.codeBackground
		border.color: Theme.border

		Item {
			id: header
			width: parent.width
			height: 34

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
				text: block.language || qsTr("text")
				color: Theme.textMuted
				font.family: Theme.monoFamily
				font.pixelSize: Theme.smallFontSize
			}

			TextButton {
				id: copyButton

				anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
				text: copiedFlag.copied ? qsTr("Copied") : qsTr("Copy")
				onClicked: {
					Clipboard.setText(source.text)
					copiedFlag.flash()
				}

				CopiedFlag { id: copiedFlag }
			}
		}

		TextEdit {
			id: source
			y: header.height
			width: parent.width
			leftPadding: 14
			rightPadding: 14
			bottomPadding: 12
			readOnly: true
			selectByMouse: true
			wrapMode: TextEdit.Wrap
			textFormat: TextEdit.PlainText
			text: block.content
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.family: Theme.monoFamily
			font.pixelSize: Theme.codeFontSize
		}

		// An empty or unknown language resolves to no definition: plain text.
		SyntaxHighlighter {
			textEdit: source
			definition: block.language
			theme: Theme.syntaxTheme
		}
	}
}
