import QtQuick
import RefGui

// Keys and commands, generated from the command registry so that it matches what really runs.
ModalDialog {
	id: dialog

	readonly property var commands: Commands.match("", false)
	readonly property var writing: [
		{title: qsTr("Send"), keys: ["Enter"]},
		{title: qsTr("New line"), keys: ["Shift", "Enter"]},
		{title: qsTr("Interrupt the session"), keys: ["Esc", "Esc"]},
		{title: qsTr("Next agent"), keys: ["Tab"]},
		{title: qsTr("Previous agent"), keys: ["Shift", "Tab"]},
		{title: qsTr("Earlier prompts"), keys: ["↑", "↓"]},
		{title: qsTr("Move between tiles (split view)"), keys: ["Alt", "←↑↓→"]}
	]

	title: qsTr("Keybindings and commands")
	dialogWidth: Theme.dialogWidthWide

	Column {
		width: parent.width
		bottomPadding: 16

		SectionLabel { text: qsTr("Keyboard") }
		Repeater {
			model: dialog.commands.filter(command => command.keys.length > 0)
			delegate: Item {
				required property var modelData
				width: parent.width
				height: 30

				Text {
					textFormat: Text.PlainText
					x: 22
					anchors.verticalCenter: parent.verticalCenter
					text: parent.modelData.title
					color: Theme.text
					font.pixelSize: Theme.smallFontSize + 1
				}
				KeyChips {
					anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
					keys: parent.modelData.keys
				}
			}
		}

		SectionLabel { text: qsTr("Writing") }
		Repeater {
			model: dialog.writing
			delegate: Item {
				required property var modelData
				width: parent.width
				height: 30

				Text {
					textFormat: Text.PlainText
					x: 22
					anchors.verticalCenter: parent.verticalCenter
					text: parent.modelData.title
					color: Theme.text
					font.pixelSize: Theme.smallFontSize + 1
				}
				KeyChips {
					anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
					keys: parent.modelData.keys
				}
			}
		}

		SectionLabel { text: qsTr("Commands") }
		Repeater {
			model: dialog.commands.filter(command => command.slash !== "")
			delegate: Item {
				id: row
				required property var modelData
				width: parent.width
				height: 30

				Text {
					textFormat: Text.PlainText
					x: 22
					anchors.verticalCenter: parent.verticalCenter
					text: row.modelData.title
					color: Theme.text
					font.pixelSize: Theme.smallFontSize + 1
				}
				Text {
					textFormat: Text.PlainText
					anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
					text: ["/" + row.modelData.slash].concat(row.modelData.aliases.map(alias => "/" + alias)).join(" ") + (row.modelData.hint !== "" ? " " + row.modelData.hint : "")
					color: Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
				}
			}
		}
	}

	component SectionLabel: Text {

		textFormat: Text.PlainText
		width: parent.width
		height: Theme.sidebarSectionHeight
		leftPadding: 22
		verticalAlignment: Text.AlignVCenter
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}
}
