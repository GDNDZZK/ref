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
		for (const command of Commands.match(input.text.trim(), false)) {
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
		close()
		Commands.run(command, args)
	}

	function choose(command) {
		if (command.hint === "") {
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
	Keys.onEscapePressed: close()

	// A click outside the panel closes the launcher.
	TapHandler { onTapped: launcher.close() }

	ListModel { id: items }

	Rectangle {
		id: panel

		readonly property bool shown: launcher.entered && launcher.open

		width: Math.min(Theme.paletteWidth, launcher.width - 2 * 24)
		height: searchRow.height + 1 + (launcher.askingArgument ? description.height : list.implicitHeight)
		x: Math.round((launcher.width - width) / 2)
		y: Math.max(48, Math.round(launcher.height * Theme.paletteTopRatio)) - (shown ? 0 : Motion.dialogDrop)
		radius: Theme.radiusLarge
		color: Theme.surface
		border.color: Theme.border
		clip: true
		opacity: shown ? 1 : 0
		scale: shown ? 1 : Motion.popScale

		Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on opacity { NumberAnimation { duration: panel.shown ? Motion.standard : Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

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
				x: 14
				anchors.verticalCenter: parent.verticalCenter
				width: chipLabel.implicitWidth + 2 * 10
				height: 26
				radius: 13
				color: Theme.userBubble
				border.color: Theme.userBubbleBorder
				visible: launcher.askingArgument

				Text {
					id: chipLabel
					anchors.centerIn: parent
					text: launcher.chosen?.title ?? ""
					color: Theme.text
					font.pixelSize: Theme.smallFontSize + 1
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
				onTextChanged: launcher.rebuild()
				Keys.onUpPressed: list.move(-1)
				Keys.onDownPressed: list.move(1)
				Keys.onReturnPressed: launcher.submit()
				Keys.onEnterPressed: launcher.submit()
				Keys.onPressed: event => {
					if (event.key === Qt.Key_Backspace && text === "" && launcher.askingArgument) {
						launcher.chosen = null
						event.accepted = true
					}
				}

				Text {
					anchors.fill: parent
					verticalAlignment: Text.AlignVCenter
					text: launcher.askingArgument ? launcher.chosen.hint : qsTr("Type a command…")
					color: Theme.textMuted
					font.pixelSize: Theme.fontSize
					visible: input.text === "" && input.preeditText === ""
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
					id: titleLabel
					anchors { left: parent.left; leftMargin: 46; verticalCenter: parent.verticalCenter }
					text: row.title
					color: Theme.text
					font.pixelSize: Theme.fontSize
				}
				Text {
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
			font.pixelSize: Theme.smallFontSize + 1
			elide: Text.ElideRight
		}
	}
}
