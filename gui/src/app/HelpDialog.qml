pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Keys and commands, generated from the command registry so that it matches what really runs.
ModalDialog {
	id: dialog

	readonly property var commands: Commands.matching("", false)
	readonly property var writing: [
		{title: qsTr("Send"), keys: ["Enter"]},
		{title: qsTr("New line"), keys: ["Shift", "Enter"]},
		{title: qsTr("Interrupt the session"), keys: ["Esc", "Esc"]},
		{title: qsTr("Next agent"), keys: ["Tab"]},
		{title: qsTr("Previous agent"), keys: ["Shift", "Tab"]},
		{title: qsTr("Earlier prompts"), keys: ["↑", "↓"]},
		{title: qsTr("Move between tiles (split view)"), keys: ["Alt", "←↑↓→"]}
	]

	// "/name /alias hint", as the user types it.
	function syntaxOf(command) {
		const names = ["/" + command.slash].concat(command.aliases.map(alias => "/" + alias))
		const hint = command.hint !== "" ? " " + command.hint : ""
		return names.join(" ") + hint
	}

	title: qsTr("Keybindings and commands")
	dialogWidth: Theme.dialogWidthWide

	Column {
		width: parent.width
		bottomPadding: 16

		SectionLabel { text: qsTr("Keyboard") }
		Repeater {
			model: dialog.commands.filter(command => command.keys.length > 0)
			delegate: HelpRow {
				id: keyRow
				required property var modelData
				title: modelData.title

				KeyChips {
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					keys: keyRow.modelData.keys
				}
			}
		}

		SectionLabel { text: qsTr("Writing") }
		Repeater {
			model: dialog.writing
			delegate: HelpRow {
				id: writeRow
				required property var modelData
				title: modelData.title

				KeyChips {
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					keys: writeRow.modelData.keys
				}
			}
		}

		SectionLabel { text: qsTr("Commands") }
		Repeater {
			model: dialog.commands.filter(command => command.slash !== "")
			delegate: HelpRow {
				id: commandRow
				required property var modelData
				title: modelData.title

				Text {
					textFormat: Text.PlainText
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					text: dialog.syntaxOf(commandRow.modelData)
					color: Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
				}
			}
		}
	}

	// A line of the lists: the title on the left, what goes on the right is the content.
	component HelpRow: Item {
		property string title: ""

		width: parent.width
		height: 30

		Text {
			textFormat: Text.PlainText
			x: Theme.dialogPadding
			anchors.verticalCenter: parent.verticalCenter
			text: parent.title
			color: Theme.text
			font.pixelSize: Theme.captionFontSize
		}
	}
}
