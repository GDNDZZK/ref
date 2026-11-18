pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Editable `string[]` list with inline row editing, deletion, and an optional shell-command
// splitter (`allowCommandSplit: true` for `mcp.<server>.command`).
Column {
	id: editor

	property var values: []
	property string placeholderText: qsTr("Add item…")
	property string addButtonText: qsTr("Add")
	property string emptyText: qsTr("No items")
	property bool monospace: false
	property bool allowCommandSplit: false
	property string draftText: ""
	property var rowDrafts: ({})

	signal valuesModified(var newValues)

	readonly property bool canSplitDraft: allowCommandSplit && /\s/.test(draftText.trim())
	readonly property bool hasValidationError: false
	readonly property bool hasUncommittedEdit: draftText.trim() !== "" || Object.keys(rowDrafts).length > 0

	function splitCommandLine(commandLine) {
		const tokens = []
		let current = ""
		let quote = ""
		let escaped = false
		for (let i = 0; i < commandLine.length; ++i) {
			const ch = commandLine[i]
			if (escaped) {
				current += ch
				escaped = false
				continue
			}
			if (ch === "\\" && quote !== "'") {
				escaped = true
				continue
			}
			if (quote !== "") {
				if (ch === quote)
					quote = ""
				else
					current += ch
				continue
			}
			if (ch === "\"" || ch === "'") {
				quote = ch
				continue
			}
			if (/\s/.test(ch)) {
				if (current !== "") {
					tokens.push(current)
					current = ""
				}
				continue
			}
			current += ch
		}
		if (current !== "")
			tokens.push(current)
		return tokens
	}

	function setRowDraft(index, text) {
		const next = Object.assign({}, rowDrafts)
		next[index] = text
		rowDrafts = next
	}

	function clearRowDraft(index) {
		if (!Fields.has(rowDrafts, index))
			return
		const next = Object.assign({}, rowDrafts)
		delete next[index]
		rowDrafts = next
	}

	function commitList(next) {
		values = next
		valuesModified(next)
	}

	function updateAt(index, text) {
		clearRowDraft(index)
		if (index < 0 || index >= values.length || values[index] === text)
			return
		const next = values.slice()
		next[index] = text
		commitList(next)
	}

	function removeAt(index) {
		rowDrafts = ({})
		if (index < 0 || index >= values.length)
			return
		const next = values.slice()
		next.splice(index, 1)
		commitList(next)
	}

	function appendDraft() {
		const trimmed = draftInput.text.trim()
		if (trimmed === "")
			return
		draftInput.text = ""
		draftText = ""
		commitList(values.concat([trimmed]))
	}

	function appendSplitCommand() {
		const parts = splitCommandLine(draftInput.text.trim())
		if (parts.length === 0)
			return
		draftInput.text = ""
		draftText = ""
		commitList(values.concat(parts))
	}

	function commitPending() {
		const indices = Object.keys(rowDrafts).map(k => Number(k)).sort((a, b) => b - a)
		for (const idx of indices) {
			const trimmed = String(rowDrafts[idx] ?? "").trim()
			if (trimmed === "")
				removeAt(idx)
			else
				updateAt(idx, trimmed)
		}
		if (draftInput.text.trim() !== "")
			appendDraft()
		return true
	}

	function resetState() {
		rowDrafts = ({})
		draftInput.text = ""
		draftText = ""
	}

	spacing: 6
	width: parent ? parent.width : 360
	opacity: enabled ? 1 : Theme.disabledOpacity

	Text {
		textFormat: Text.PlainText
		visible: editor.values.length === 0
		width: parent.width
		text: editor.emptyText
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		elide: Text.ElideRight
	}

	Repeater {
		model: editor.values

		delegate: Item {
			id: itemRow
			required property string modelData
			required property int index

			width: editor.width
			height: 34

			Rectangle {
				anchors { left: parent.left; right: removeBtn.left; rightMargin: 6; top: parent.top; bottom: parent.bottom }
				radius: Theme.radiusSmall + 2
				color: Theme.background
				border.color: rowInput.activeFocus ? Theme.accentText : Theme.border

				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				TextInput {
					id: rowInput
					anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
					verticalAlignment: TextInput.AlignVCenter
					text: itemRow.modelData
					color: Theme.text
					selectionColor: Theme.selection
					selectedTextColor: Theme.text
					font {
						family: editor.monospace ? Theme.monoFamily : ""
						pixelSize: editor.monospace ? Theme.codeFontSize : Theme.secondaryFontSize
					}
					clip: true
					onTextEdited: editor.setRowDraft(itemRow.index, text)
					onEditingFinished: {
						const trimmed = text.trim()
						if (trimmed === "")
							editor.removeAt(itemRow.index)
						else
							editor.updateAt(itemRow.index, trimmed)
					}
				}
			}

			IconButton {
				id: removeBtn
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				buttonSize: 28
				iconName: "close"
				iconSize: 16
				Accessible.name: qsTr("Remove item")
				onClicked: editor.removeAt(itemRow.index)
			}
		}
	}

	Item {
		width: editor.width
		height: Theme.fieldHeight

		Rectangle {
			anchors { left: parent.left; right: addControls.left; rightMargin: 8; top: parent.top; bottom: parent.bottom }
			radius: Theme.radius
			color: Theme.background
			border.color: draftInput.activeFocus ? Theme.accentText : Theme.border

			Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			TextInput {
				id: draftInput
				anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
				verticalAlignment: TextInput.AlignVCenter
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font {
					family: editor.monospace ? Theme.monoFamily : ""
					pixelSize: editor.monospace ? Theme.codeFontSize : Theme.secondaryFontSize
				}
				clip: true
				Accessible.name: editor.placeholderText
				onTextEdited: editor.draftText = text
				onAccepted: editor.appendDraft()

				PlaceholderText {
					anchors.fill: parent
					textInput: draftInput
					text: editor.placeholderText
					font.pixelSize: Theme.secondaryFontSize
					elide: Text.ElideRight
				}
			}
		}

		Row {
			id: addControls
			anchors { right: parent.right; verticalCenter: parent.verticalCenter }
			spacing: 6

			TextButton {
				visible: editor.canSplitDraft
				anchors.verticalCenter: parent.verticalCenter
				text: qsTr("Split command")
				onClicked: editor.appendSplitCommand()
			}

			DialogButton {
				anchors.verticalCenter: parent.verticalCenter
				text: editor.addButtonText
				enabled: editor.enabled && editor.draftText.trim() !== ""
				onClicked: editor.appendDraft()
			}
		}
	}
}
