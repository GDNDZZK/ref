pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// One choice of a question: its number, a radio or check mark, the label and what it means.
// The custom row turns into a text field while the user types an answer of their own.
Item {
	id: row

	required property int index
	required property string label
	required property string description
	required property bool isDefault
	required property bool checked
	required property bool highlighted
	required property int number
	required property bool custom
	required property bool multiple
	// The card is typing a custom answer; only the custom row shows the field.
	required property bool editing
	required property string customText
	required property bool busy

	signal activated()
	signal hovered()
	signal committed(string text)
	signal cancelled()

	readonly property int rowPadding: 10
	readonly property int rowSpacing: 10
	readonly property int markSize: 16
	readonly property int numberWidth: 14
	readonly property int verticalPadding: 6
	readonly property real highlightOpacity: 0.12
	readonly property real highlightBorderOpacity: 0.5
	readonly property int checkRadius: 4
	readonly property int textSpacing: 2
	readonly property real busyOpacity: 0.5
	readonly property bool typing: custom && editing

	implicitHeight: content.height + 2 * verticalPadding
	opacity: busy ? busyOpacity : 1

	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: row.highlighted ? Qt.alpha(Theme.accent, row.highlightOpacity) : "transparent"
		border.color: row.highlighted ? Qt.alpha(Theme.accent, row.highlightBorderOpacity) : "transparent"
	}

	Row {
		id: content
		x: row.rowPadding
		y: row.verticalPadding
		width: parent.width - 2 * row.rowPadding
		spacing: row.rowSpacing

		Text {
			textFormat: Text.PlainText
			width: row.numberWidth
			text: row.number
			color: row.highlighted ? Theme.accent : Theme.textMuted
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
			horizontalAlignment: Text.AlignRight
			height: mark.height
			verticalAlignment: Text.AlignVCenter
		}

		// A square when several may be ticked, a circle when one is picked.
		Rectangle {
			id: mark
			width: row.markSize
			height: row.markSize
			radius: row.multiple ? row.checkRadius : height / 2
			color: row.checked && row.multiple ? Theme.accent : "transparent"
			border.color: row.checked ? Theme.accent : Theme.textMuted
			border.width: 1

			Icon {
				anchors.centerIn: parent
				visible: row.checked && row.multiple
				name: "check"
				color: Theme.onAccent
			}
			Rectangle {
				anchors.centerIn: parent
				visible: row.checked && !row.multiple
				width: parent.width / 2
				height: width
				radius: width / 2
				color: Theme.accent
			}
		}

		Column {
			width: parent.width - row.numberWidth - row.markSize - 2 * row.rowSpacing
			spacing: row.textSpacing

			Text {
				id: labelText
				textFormat: Text.PlainText
				visible: !row.typing
				width: parent.width
				text: row.isDefault ? qsTr("%1 (default)").arg(row.label) : row.label
				color: row.custom && row.customText === "" ? Theme.textMuted : Theme.text
				font.pixelSize: Theme.fontSize - 1
				font.italic: row.custom && row.customText === ""
				wrapMode: Text.Wrap
			}
			Text {
				textFormat: Text.PlainText
				visible: row.description !== "" && !row.typing
				width: parent.width
				text: row.description
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				wrapMode: Text.Wrap
			}

			Rectangle {
				id: field
				visible: row.typing
				width: parent.width
				height: input.implicitHeight + 2 * row.verticalPadding
				radius: Theme.radiusSmall
				color: Theme.background
				border.color: Theme.accent

				TextInput {
					id: input
					anchors { fill: parent; leftMargin: row.rowPadding; rightMargin: row.rowPadding }
					verticalAlignment: TextInput.AlignVCenter
					color: Theme.text
					selectionColor: Theme.selection
					selectedTextColor: Theme.text
					font.pixelSize: Theme.fontSize - 1
					clip: true
					Accessible.name: qsTr("Your answer")

					Keys.onReturnPressed: row.committed(text)
					Keys.onEnterPressed: row.committed(text)
					Keys.onEscapePressed: row.cancelled()
				}
			}
		}
	}

	// The field starts with what was typed before, selected.
	onTypingChanged: {
		if (!typing)
			return
		input.text = customText
		input.forceActiveFocus()
		input.selectAll()
	}

	HoverHandler {
		cursorShape: Qt.PointingHandCursor
		enabled: !row.typing
		onHoveredChanged: if (hovered) row.hovered()
	}
	TapHandler {
		enabled: !row.typing
		onTapped: row.activated()
	}

	Accessible.role: multiple ? Accessible.CheckBox : Accessible.RadioButton
	Accessible.name: label
	Accessible.checked: checked
	Accessible.onPressAction: activated()
}
