pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Key-value map editor for HTTP headers, MCP environment variables, tool delays (`valueMode: "number"`),
// and request/variant options (`valueMode: "json"`).
Column {
	id: kvEditor

	// Array of `{ key: string, value: any }`. Can also be populated via `loadFromMap(obj)`.
	property var entries: []
	// `"string"`, `"number"`, or `"json"`.
	property string valueMode: "string"
	property string keyPlaceholder: qsTr("Key")
	property string valuePlaceholder: qsTr("Value")
	property string addButtonText: qsTr("Add")
	property string emptyText: qsTr("No entries")
	property var envStatus: ({})

	signal entriesModified(var newEntries)
	signal mapModified(var newMap)

	function formatCellValue(raw) {
		if (valueMode === "json") {
			if (typeof raw === "string")
				return raw
			return JSON.stringify(raw)
		}
		return raw === undefined || raw === null ? "" : String(raw)
	}

	function parseCellValue(rawText) {
		if (valueMode === "number") {
			const num = Number(rawText.trim())
			return Number.isFinite(num) ? num : 0
		}
		if (valueMode === "json") {
			const trimmed = rawText.trim()
			if (trimmed === "")
				return ""
			try {
				return JSON.parse(trimmed)
			} catch (err) {
				return rawText
			}
		}
		return rawText
	}

	function extractEnvVars(text) {
		const vars = []
		let rest = String(text ?? "")
		while (rest.length > 0) {
			const idx = rest.indexOf("{env:")
			if (idx < 0)
				break
			const slice = rest.slice(idx)
			const closeIdx = slice.indexOf("}")
			if (closeIdx < 0)
				break
			const name = slice.slice(5, closeIdx).trim()
			if (name !== "" && !vars.includes(name))
				vars.push(name)
			rest = slice.slice(closeIdx + 1)
		}
		return vars
	}

	function loadFromMap(mapObj) {
		if (!mapObj || typeof mapObj !== "object") {
			entries = []
			return
		}
		const loaded = []
		for (const k of Object.keys(mapObj)) {
			if (Fields.has(mapObj, k))
				loaded.push({ key: k, value: mapObj[k] })
		}
		entries = loaded
	}

	function toMap(list) {
		const out = {}
		for (const item of list) {
			const k = String(item.key ?? "").trim()
			if (k !== "")
				out[k] = item.value
		}
		return out
	}

	function commitEntries(next) {
		entries = next
		entriesModified(next)
		mapModified(toMap(next))
	}

	function updateRow(index, newKey, newValText) {
		if (index < 0 || index >= entries.length)
			return
		const trimmedKey = newKey.trim()
		if (trimmedKey === "") {
			removeRow(index)
			return
		}
		const parsedVal = parseCellValue(newValText)
		const next = entries.slice()
		next[index] = { key: trimmedKey, value: parsedVal }
		commitEntries(next)
	}

	function removeRow(index) {
		if (index < 0 || index >= entries.length)
			return
		const next = entries.slice()
		next.splice(index, 1)
		commitEntries(next)
	}

	function addDraft() {
		const k = newKeyInput.text.trim()
		if (k === "")
			return
		const v = parseCellValue(newValInput.text)
		const next = entries.filter(item => item.key !== k)
		next.push({ key: k, value: v })
		commitEntries(next)
		newKeyInput.text = ""
		newValInput.text = ""
		newKeyInput.forceActiveFocus()
	}

	spacing: 6
	width: parent ? parent.width : 420
	opacity: enabled ? 1 : Theme.disabledOpacity

	Text {
		textFormat: Text.PlainText
		visible: kvEditor.entries.length === 0
		width: parent.width
		text: kvEditor.emptyText
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		elide: Text.ElideRight
	}

	Repeater {
		model: kvEditor.entries

		delegate: Item {
			id: kvRow
			required property var modelData
			required property int index

			readonly property var rowEnvVars: kvEditor.valueMode === "string"
				? kvEditor.extractEnvVars(valInput.text)
				: []
			readonly property var missingVars: rowEnvVars.filter(v =>
				Fields.has(kvEditor.envStatus, v) && kvEditor.envStatus[v] === false)
			readonly property bool hasEnvBadge: rowEnvVars.length > 0
				&& (rowEnvVars.every(v => Fields.has(kvEditor.envStatus, v)) || missingVars.length > 0)
			readonly property bool envOk: missingVars.length === 0 && hasEnvBadge
			readonly property real keyBoxWidth: Math.max(100, Math.floor((width - 40) * 0.38))

			width: kvEditor.width
			height: 34

			Rectangle {
				id: keyBox
				width: kvRow.keyBoxWidth
				height: parent.height
				radius: Theme.radiusSmall + 2
				color: Theme.background
				border.color: keyInput.activeFocus ? Theme.accentText : Theme.border

				TextInput {
					id: keyInput
					anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
					verticalAlignment: TextInput.AlignVCenter
					text: String(kvRow.modelData.key ?? "")
					color: Theme.text
					selectionColor: Theme.selection
					selectedTextColor: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
					clip: true
					onEditingFinished: kvEditor.updateRow(kvRow.index, text, valInput.text)
				}
			}

			Rectangle {
				anchors { left: keyBox.right; leftMargin: 6; right: deleteBtn.left; rightMargin: 6; top: parent.top; bottom: parent.bottom }
				radius: Theme.radiusSmall + 2
				color: Theme.background
				border.color: valInput.activeFocus ? Theme.accentText : Theme.border

				TextInput {
					id: valInput
					anchors {
						left: parent.left
						leftMargin: 10
						right: badgeDot.visible ? badgeDot.left : parent.right
						rightMargin: 8
						top: parent.top
						bottom: parent.bottom
					}
					verticalAlignment: TextInput.AlignVCenter
					text: kvEditor.formatCellValue(kvRow.modelData.value)
					color: Theme.text
					selectionColor: Theme.selection
					selectedTextColor: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
					clip: true
					onEditingFinished: kvEditor.updateRow(kvRow.index, keyInput.text, text)
				}

				Icon {
					id: badgeDot
					visible: kvRow.hasEnvBadge
					anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					name: kvRow.envOk ? "check" : "warning"
					size: 16
					color: kvRow.envOk ? Theme.successText : Theme.warningText

					HoverHandler { id: badgeHover }
					PlainToolTip {
						visible: badgeHover.hovered
						text: kvRow.envOk
							? qsTr("Environment variable set: %1").arg(kvRow.rowEnvVars.join(", "))
							: qsTr("Environment variable unset: %1").arg(kvRow.missingVars.join(", "))
					}
				}
			}

			IconButton {
				id: deleteBtn
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				buttonSize: 28
				iconName: "close"
				iconSize: 16
				Accessible.name: qsTr("Remove entry")
				onClicked: kvEditor.removeRow(kvRow.index)
			}
		}
	}

	Item {
		width: kvEditor.width
		height: Theme.fieldHeight
		readonly property real draftKeyWidth: Math.max(100, Math.floor((width - addBtn.width - 14) * 0.38))

		Rectangle {
			id: draftKeyBox
			width: parent.draftKeyWidth
			height: parent.height
			radius: Theme.radius
			color: Theme.background
			border.color: newKeyInput.activeFocus ? Theme.accentText : Theme.border

			TextInput {
				id: newKeyInput
				anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
				verticalAlignment: TextInput.AlignVCenter
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
				clip: true
				Accessible.name: kvEditor.keyPlaceholder
				onAccepted: newValInput.forceActiveFocus()

				PlaceholderText {
					anchors.fill: parent
					textInput: newKeyInput
					text: kvEditor.keyPlaceholder
					font.pixelSize: Theme.secondaryFontSize
					elide: Text.ElideRight
				}
			}
		}

		Rectangle {
			anchors { left: draftKeyBox.right; leftMargin: 6; right: addBtn.left; rightMargin: 8; top: parent.top; bottom: parent.bottom }
			radius: Theme.radius
			color: Theme.background
			border.color: newValInput.activeFocus ? Theme.accentText : Theme.border

			TextInput {
				id: newValInput
				anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
				verticalAlignment: TextInput.AlignVCenter
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
				clip: true
				Accessible.name: kvEditor.valuePlaceholder
				onAccepted: kvEditor.addDraft()

				PlaceholderText {
					anchors.fill: parent
					textInput: newValInput
					text: kvEditor.valuePlaceholder
					font.pixelSize: Theme.secondaryFontSize
					elide: Text.ElideRight
				}
			}
		}

		DialogButton {
			id: addBtn
			anchors { right: parent.right; verticalCenter: parent.verticalCenter }
			text: kvEditor.addButtonText
			enabled: kvEditor.enabled && newKeyInput.text.trim() !== ""
			onClicked: kvEditor.addDraft()
		}
	}
}
