pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// The model's question to the user, above the input box: one list to pick from, or a tab per
// question with a last tab to check the answers. The keys come from the session pane (see
// ChatSession.answerKey); the mouse works on its own.
Rectangle {
	id: card

	required property QuestionPrompt prompt
	// The most the card may take; the choices scroll beyond it.
	property real maxHeight: 400

	readonly property int padding: 14
	readonly property int spacing: 8
	readonly property int tabHeight: 26
	readonly property int tabPadding: 10
	readonly property real borderOpacity: 0.6
	readonly property real busyOpacity: 0.5
	readonly property string hint: {
		if (prompt.busy)
			return qsTr("Sending…")
		if (prompt.editing)
			return prompt.simple ? qsTr("Enter submit · Esc back") : qsTr("Enter done · Esc back")
		if (prompt.simple)
			return qsTr("↑↓ select · Enter submit · Esc dismiss")
		if (prompt.confirming)
			return qsTr("←→ tab · Enter submit · Esc dismiss")
		return prompt.multiple ? qsTr("←→ tab · ↑↓ select · Enter toggle · Esc dismiss") : qsTr("←→ tab · ↑↓ select · Enter select · Esc dismiss")
	}
	readonly property real fixedHeight: 2 * padding + heading.height + footer.height + 2 * spacing

	visible: prompt.active
	height: prompt.active ? Math.min(fixedHeight + choices.contentHeight, maxHeight) : 0
	radius: Theme.radius
	color: Theme.surface
	border.color: Qt.alpha(Theme.accent, borderOpacity)

	Column {
		id: heading
		x: card.padding
		y: card.padding
		width: parent.width - 2 * card.padding
		spacing: card.spacing

		Row {
			visible: !card.prompt.simple
			spacing: 6

			Repeater {
				model: card.prompt.tabs

				Rectangle {
					id: tab

					required property int index
					required property var modelData

					width: tabLabel.implicitWidth + 2 * card.tabPadding
					height: card.tabHeight
					radius: height / 2
					color: modelData.current ? Theme.accent : "transparent"
					border.color: modelData.current ? "transparent" : Theme.border

					Text {
						id: tabLabel
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: (tab.modelData.answered ? "✓ " : "") + tab.modelData.title
						color: tab.modelData.current ? Theme.onAccent : tab.modelData.answered ? Theme.text : Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}

					TapHandler { enabled: !card.prompt.busy; onTapped: card.prompt.goToPage(tab.index) }
					HoverHandler { cursorShape: Qt.PointingHandCursor }
					Accessible.role: Accessible.PageTab
					Accessible.name: modelData.title
				}
			}
		}

		Text {
			textFormat: Text.PlainText
			width: parent.width
			text: "? " + card.prompt.header
			color: Theme.info
			font.pixelSize: Theme.fontSize
			font.bold: true
			elide: Text.ElideRight
		}

		Text {
			textFormat: Text.PlainText
			width: parent.width
			text: card.prompt.text
			color: Theme.text
			font.pixelSize: Theme.fontSize - 1
			wrapMode: Text.Wrap
		}
	}

	Flickable {
		id: choices
		x: card.padding / 2
		y: card.padding + heading.height + card.spacing
		width: parent.width - card.padding
		height: Math.max(0, card.height - card.fixedHeight)
		contentWidth: width
		contentHeight: list.height
		interactive: contentHeight > height
		boundsBehavior: Flickable.StopAtBounds
		clip: true
		opacity: card.prompt.busy ? card.busyOpacity : 1

		// Keeps the highlighted row in view when the arrows go beyond the visible rows.
		function reveal(y, height) {
			if (y < contentY)
				contentY = y
			else if (y + height > contentY + this.height)
				contentY = y + height - this.height
		}

		Column {
			id: list
			width: parent.width

			Repeater {
				model: card.prompt.rows

				QuestionRow {
					id: choice

					required property var modelData

					width: list.width
					label: modelData.label
					description: modelData.description
					isDefault: modelData.isDefault
					checked: modelData.checked
					highlighted: modelData.highlighted
					number: modelData.number
					custom: modelData.custom
					multiple: card.prompt.multiple
					editing: card.prompt.editing
					customText: card.prompt.customText
					busy: card.prompt.busy

					onHighlightedChanged: if (highlighted) choices.reveal(y, height)
					onActivated: card.prompt.activateRow(index)
					onHovered: card.prompt.setHighlight(index)
					onCommitted: text => card.prompt.commitCustom(text)
					onCancelled: card.prompt.cancelCustom()
				}
			}

			Repeater {
				model: card.prompt.summary

				Row {
					id: entry

					required property var modelData

					width: list.width
					leftPadding: card.padding / 2
					spacing: card.spacing

					Text {
						id: entryHeader
						textFormat: Text.PlainText
						text: entry.modelData.header
						color: Theme.textMuted
						font.pixelSize: Theme.fontSize - 1
					}
					Text {
						textFormat: Text.PlainText
						width: entry.width - entry.leftPadding - entryHeader.width - entry.spacing
						text: entry.modelData.answer
						color: entry.modelData.answered ? Theme.text : Theme.warning
						font.pixelSize: Theme.fontSize - 1
						wrapMode: Text.Wrap
					}
				}
			}
		}
	}

	Item {
		id: footer
		x: card.padding
		y: card.height - card.padding - height
		width: parent.width - 2 * card.padding
		height: Theme.buttonHeight

		Text {
			textFormat: Text.PlainText
			anchors { left: parent.left; right: buttons.left; rightMargin: card.spacing; verticalCenter: parent.verticalCenter }
			text: card.hint
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			elide: Text.ElideRight
		}

		Row {
			id: buttons
			anchors { right: parent.right; verticalCenter: parent.verticalCenter }
			spacing: card.spacing

			DialogButton {
				small: true
				text: qsTr("Dismiss")
				enabled: !card.prompt.busy
				onClicked: card.prompt.dismiss()
			}
			DialogButton {
				small: true
				kind: "primary"
				visible: card.prompt.confirming
				text: qsTr("Submit")
				enabled: !card.prompt.busy
				onClicked: card.prompt.submit()
			}
		}
	}

	Accessible.role: Accessible.Dialog
	Accessible.name: prompt.header
}
