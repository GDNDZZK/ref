pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Ctrl+P: every command, searched as you type. A command that takes an argument turns the
// search row into an argument prompt.
FocusScope {
	id: launcher

	readonly property bool open: Workspace.dialog !== ""
	readonly property bool askingArgument: chosen !== null
	property bool entered: false
	// The command waiting for its argument, or null.
	property var chosen: null

	function close() {
		Workspace.closeDialog()
	}

	function iconName(icon) {
		return icon.replace(/-\d+$/, "")
	}

	function iconSize(icon) {
		return parseInt(icon.match(/-(\d+)$/)?.[1] ?? "16")
	}

	function rebuild() {
		items.clear()
		if (askingArgument)
			return
		for (const command of Commands.matching(input.text.trim(), false)) {
			items.append({
				commandId: command.id,
				title: command.title,
				slash: command.slash,
				hint: command.hint,
				keys: command.keys.join("|"),
				category: command.category,
				icon: command.icon,
				usable: command.enabled
			})
		}
		list.reset()
	}

	function run(command, args) {
		Commands.run(command, args)
		if (Workspace.dialog === DialogId.palette)
			close()
	}

	readonly property var dialogCommands: [
		"sessions", "models", "agents", "variant", "rename", "cd", "close-session", "export"
	]

	function choose(command) {
		if (command.hint === "" || dialogCommands.includes(command.commandId)) {
			run(command.commandId, "")
			return
		}
		chosen = {id: command.commandId, title: command.title, hint: command.hint}
		input.text = ""
	}

	function submit() {
		if (askingArgument)
			run(chosen.id, input.text.trim())
		else
			list.activateCurrent()
	}

	anchors.fill: parent
	focus: true

	Component.onCompleted: {
		rebuild()
		entered = true
		input.forceActiveFocus()
	}
	onChosenChanged: rebuild()
	Keys.onEscapePressed: event => {
		if (askingArgument) {
			chosen = null
			input.text = ""
			event.accepted = true
			return
		}
		close()
	}

	// A click outside the panel closes the launcher.
	TapHandler { onTapped: launcher.close() }

	Connections {
		target: Commands
		function onModelReset() { launcher.rebuild() }
		function onRowsInserted() { launcher.rebuild() }
		function onRowsRemoved() { launcher.rebuild() }
		function onDataChanged() { launcher.rebuild() }
	}

	ListModel { id: items }

	Rectangle {
		id: panel

		readonly property bool shown: launcher.entered && launcher.open
		property bool heightReady: false
		property real dropOffset: shown ? 0 : Motion.dialogDrop

		width: Math.min(Theme.paletteWidth, launcher.width - 2 * Theme.overlayEdgeMargin)
		height: searchRow.height + 1 + (launcher.askingArgument ? description.height : list.implicitHeight)
		x: Math.round((launcher.width - width) / 2)
		y: Math.max(48, Math.round(launcher.height * Theme.paletteTopRatio) - Math.round(dropOffset))
		radius: Theme.radiusLarge
		color: Theme.surface
		border.color: Theme.border
		clip: true
		opacity: shown ? 1 : 0
		scale: shown ? 1 : Motion.popScale

		Timer {
			interval: 0
			running: launcher.entered && launcher.height > 0 && !panel.heightReady
			onTriggered: panel.heightReady = true
		}

		Behavior on dropOffset { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on opacity { NumberAnimation { duration: panel.shown ? Motion.standard : Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on height {
			enabled: panel.heightReady
			NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
		}
		TapHandler { gesturePolicy: TapHandler.ReleaseWithinBounds }

		Item {
			id: searchRow
			width: parent.width
			height: Theme.paletteSearchHeight

			Icon {
				x: 18
				anchors.verticalCenter: parent.verticalCenter
				name: "search"
				size: 16
				color: Theme.textMuted
				visible: !launcher.askingArgument
			}

			Rectangle {
				id: chip
				readonly property int maxChipWidth: Math.max(80, Math.floor(searchRow.width * 0.45))
				x: 14
				anchors.verticalCenter: parent.verticalCenter
				width: Math.min(chipLabel.implicitWidth + 2 * 10, maxChipWidth)
				height: 26
				radius: 13
				color: Theme.userBubble
				border.color: Theme.userBubbleBorder
				visible: launcher.askingArgument

				Text {
					textFormat: Text.PlainText
					id: chipLabel
					anchors { left: parent.left; right: parent.right; leftMargin: 10; rightMargin: 10; verticalCenter: parent.verticalCenter }
					text: launcher.chosen?.title ?? ""
					color: Theme.text
					font.pixelSize: Theme.captionFontSize
					elide: Text.ElideRight
				}
			}

			TextInput {
				id: input
				anchors { left: parent.left; leftMargin: launcher.askingArgument ? chip.width + 26 : 46; right: parent.right; rightMargin: 18; verticalCenter: parent.verticalCenter }
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font.pixelSize: Theme.fontSize
				clip: true
				Accessible.name: qsTr("Command")
				focus: true
				onTextChanged: launcher.rebuild()
				Keys.onUpPressed: list.move(-1)
				Keys.onDownPressed: list.move(1)
				Keys.onReturnPressed: launcher.submit()
				Keys.onEnterPressed: launcher.submit()
				Keys.onPressed: event => {
					if (event.key === Qt.Key_Escape && launcher.askingArgument) {
						launcher.chosen = null
						text = ""
						event.accepted = true
						return
					}
					if (event.key === Qt.Key_Backspace && text === "" && launcher.askingArgument) {
						launcher.chosen = null
						event.accepted = true
					}
				}

				PlaceholderText {
					anchors.fill: parent
					textInput: input
					text: launcher.askingArgument ? launcher.chosen.hint : qsTr("Type a command…")
				}
			}
		}

		Rectangle {
			id: divider
			y: searchRow.height
			width: parent.width
			height: 1
			color: Theme.border
		}

		PickList {
			id: list
			y: divider.y + 1
			width: parent.width
			searchable: false
			visible: !launcher.askingArgument
			visibleHeight: Theme.paletteMaxRows * Theme.paletteRowHeight
			sectionProperty: input.text.trim() === "" ? "category" : ""
			emptyText: qsTr("No matching commands")
			model: items
			rowEnabled: index => index >= 0 && index < items.count && items.get(index).usable
			delegate: PickRow {
				id: row
				required property string commandId
				required property string title
				required property string slash
				required property string hint
				required property string keys
				required property string icon
				required property bool usable
				height: Theme.paletteRowHeight
				enabled: usable
				onActivated: launcher.choose({commandId: commandId, title: title, hint: hint})

				Icon {
					x: 20
					anchors.verticalCenter: parent.verticalCenter
					name: launcher.iconName(row.icon)
					size: launcher.iconSize(row.icon)
					color: Theme.textMuted
				}
				Text {
					textFormat: Text.PlainText
					id: titleLabel
					anchors { left: parent.left; leftMargin: 46; verticalCenter: parent.verticalCenter }
					width: Math.min(implicitWidth, Math.max(0, chips.x - 46 - (chips.width > 0 ? 8 : 0)))
					text: row.title
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}
				Text {
					textFormat: Text.PlainText
					anchors { left: titleLabel.right; leftMargin: 8; right: chips.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
					text: row.slash !== "" ? "/" + row.slash : ""
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
				KeyChips {
					id: chips
					anchors { right: parent.right; rightMargin: 20; verticalCenter: parent.verticalCenter }
					keys: row.keys === "" ? [] : row.keys.split("|")
				}
			}
		}

		Text {
			textFormat: Text.PlainText
			id: description
			y: divider.y + 1
			width: parent.width
			height: 48
			leftPadding: 18
			rightPadding: 18
			verticalAlignment: Text.AlignVCenter
			visible: launcher.askingArgument
			text: qsTr("Type the argument and press Enter to run \"%1\".").arg(launcher.chosen?.title ?? "")
			color: Theme.textMuted
			font.pixelSize: Theme.captionFontSize
			elide: Text.ElideRight
		}
	}
}
