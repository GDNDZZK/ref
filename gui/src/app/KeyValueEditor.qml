pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Key-value map editor for HTTP headers, MCP environment variables, tool delays (`valueMode: "number"`),
// and request/variant options (`valueMode: "json"`).
Column {
	id: kvEditor

	// Array of `{ key: string, value: any, json?: string }`.
	property var entries: []
	// `"string"`, `"number"`, or `"json"`.
	property string valueMode: "string"
	property bool allowReorder: false
	property real minimum: -Infinity
	property string keyPlaceholder: qsTr("Key")
	property string valuePlaceholder: qsTr("Value")
	property string addButtonText: qsTr("Add")
	property string emptyText: qsTr("No entries")
	property var envStatus: ({})

	property var rowErrors: ({})
	property var rowDrafts: ({})
	property string draftKeyText: ""
	property string draftValText: ""
	property string draftError: ""

	signal entriesModified(var newEntries)
	signal mapModified(var newMap)

	readonly property bool hasValidationError: Object.keys(rowErrors).length > 0 || draftError !== ""
	readonly property bool hasUncommittedEdit: Object.keys(rowDrafts).length > 0
		|| draftKeyText.trim() !== ""
		|| draftValText.trim() !== ""

	function formatEntryValue(entry) {
		if (!entry || typeof entry !== "object")
			return ""
		if (valueMode === "json") {
			if (typeof entry.json === "string" && entry.json !== "")
				return entry.json
			if (entry.value === undefined)
				return "null"
			return JSON.stringify(entry.value)
		}
		const raw = entry.value
		return raw === undefined || raw === null ? "" : String(raw)
	}

	function validateAndParseCell(rawText) {
		if (valueMode === "number") {
			const trimmed = rawText.trim()
			if (trimmed === "" || !/^-?(?:\d+\.?\d*|\.\d+)$/.test(trimmed))
				return { ok: false, value: 0, error: qsTr("Enter a valid number.") }
			const num = Number(trimmed)
			if (!Number.isFinite(num))
				return { ok: false, value: 0, error: qsTr("Enter a valid number.") }
			if (num < minimum)
				return { ok: false, value: 0, error: qsTr("Must be at least %1.").arg(minimum) }
			return { ok: true, value: num, error: "" }
		}
		if (valueMode === "json") {
			const trimmed = rawText.trim()
			if (trimmed === "")
				return { ok: false, value: null, error: qsTr("Enter a JSON value (quote strings, e.g. \"text\").") }
			try {
				const parsed = JSON.parse(trimmed)
				return { ok: true, value: parsed, json: trimmed, error: "" }
			} catch (err) {
				return { ok: false, value: null, error: qsTr("Invalid JSON value (quote strings, e.g. \"text\").") }
			}
		}
		return { ok: true, value: rawText, error: "" }
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

	function loadFromEntries(list) {
		rowDrafts = ({})
		rowErrors = ({})
		if (!Fields.isArray(list)) {
			entries = []
			return
		}
		entries = Fields.toArray(list).map(item => ({
			key: String(item.key ?? ""),
			value: item.value,
			json: typeof item.json === "string" ? item.json : undefined
		}))
	}

	function loadFromMap(mapObj) {
		rowDrafts = ({})
		rowErrors = ({})
		if (!mapObj || typeof mapObj !== "object") {
			entries = []
			return
		}
		const loaded = []
		for (const k of Object.keys(mapObj)) {
			if (!Fields.has(mapObj, k))
				continue
			const val = mapObj[k]
			loaded.push({
				key: k,
				value: val,
				json: valueMode === "json" ? JSON.stringify(val) : undefined
			})
		}
		entries = loaded
	}

	function resetState() {
		rowDrafts = ({})
		rowErrors = ({})
		draftError = ""
		draftKeyText = ""
		draftValText = ""
		newKeyInput.text = ""
		newValInput.text = ""
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

	function setRowError(index, msg) {
		const next = Object.assign({}, rowErrors)
		if (msg === "")
			delete next[index]
		else
			next[index] = msg
		rowErrors = next
	}

	function setRowDraft(index, keyText, valText) {
		const next = Object.assign({}, rowDrafts)
		next[index] = { key: keyText, valText: valText }
		rowDrafts = next
	}

	function clearRowDraft(index) {
		if (!Fields.has(rowDrafts, index))
			return
		const next = Object.assign({}, rowDrafts)
		delete next[index]
		rowDrafts = next
	}

	function hasDuplicateKey(candidateKey, ignoreIndex) {
		return entries.some((item, idx) => idx !== ignoreIndex && item.key === candidateKey)
	}

	function onRowEdited(index, keyText, valText) {
		setRowDraft(index, keyText, valText)
		const trimmedKey = keyText.trim()
		if (trimmedKey === "") {
			setRowError(index, "")
			return
		}
		if (hasDuplicateKey(trimmedKey, index)) {
			setRowError(index, qsTr("Key \"%1\" already exists.").arg(trimmedKey))
			return
		}
		const checked = validateAndParseCell(valText)
		setRowError(index, checked.ok ? "" : checked.error)
	}

	function commitEntries(next) {
		entries = next
		entriesModified(next)
		mapModified(toMap(next))
	}

	function updateRow(index, newKey, newValText) {
		if (index < 0 || index >= entries.length)
			return false
		const trimmedKey = newKey.trim()
		if (trimmedKey === "") {
			clearRowDraft(index)
			setRowError(index, "")
			removeRow(index)
			return true
		}
		if (hasDuplicateKey(trimmedKey, index)) {
			setRowError(index, qsTr("Key \"%1\" already exists.").arg(trimmedKey))
			return false
		}
		const checked = validateAndParseCell(newValText)
		if (!checked.ok) {
			setRowError(index, checked.error)
			return false
		}
		clearRowDraft(index)
		setRowError(index, "")
		const nextEntry = {
			key: trimmedKey,
			value: checked.value,
			json: checked.json
		}
		const prev = entries[index]
		const prevFormatted = formatEntryValue(prev)
		const nextFormatted = formatEntryValue(nextEntry)
		if (prev.key === trimmedKey && prevFormatted === nextFormatted && prev.value === checked.value)
			return true
		const next = entries.slice()
		next[index] = nextEntry
		commitEntries(next)
		return true
	}

	function moveRow(index, delta) {
		const target = index + delta
		if (index < 0 || index >= entries.length || target < 0 || target >= entries.length)
			return
		rowDrafts = ({})
		rowErrors = ({})
		const next = entries.slice()
		const moved = next[index]
		next[index] = next[target]
		next[target] = moved
		commitEntries(next)
	}

	function removeRow(index) {
		if (index < 0 || index >= entries.length)
			return
		rowDrafts = ({})
		rowErrors = ({})
		const next = entries.slice()
		next.splice(index, 1)
		commitEntries(next)
	}

	function updateDraftValidation() {
		draftKeyText = newKeyInput.text
		draftValText = newValInput.text
		if (draftKeyText.trim() === "" && draftValText.trim() === "") {
			draftError = ""
			return
		}
		if (draftKeyText.trim() !== "" && valueMode !== "string" && draftValText.trim() !== "") {
			const checked = validateAndParseCell(draftValText)
			draftError = checked.ok ? "" : checked.error
			return
		}
		draftError = ""
	}

	function tryCommitDraft(refocusKey) {
		const k = newKeyInput.text.trim()
		const rawVal = newValInput.text
		if (k === "" && rawVal.trim() === "") {
			draftError = ""
			return true
		}
		if (k === "") {
			draftError = qsTr("Enter a key.")
			return false
		}
		const checked = validateAndParseCell(rawVal)
		if (!checked.ok) {
			draftError = checked.error
			return false
		}
		draftError = ""
		const newEntry = { key: k, value: checked.value, json: checked.json }
		const existingIdx = entries.findIndex(item => item.key === k)
		const next = entries.slice()
		if (existingIdx >= 0)
			next[existingIdx] = newEntry
		else
			next.push(newEntry)
		newKeyInput.text = ""
		newValInput.text = ""
		draftKeyText = ""
		draftValText = ""
		commitEntries(next)
		if (refocusKey)
			newKeyInput.forceActiveFocus()
		return true
	}

	function addDraft() {
		tryCommitDraft(true)
	}

	function commitPending() {
		const draftIndices = Object.keys(rowDrafts).map(k => Number(k)).sort((a, b) => b - a)
		for (const idx of draftIndices) {
			const d = rowDrafts[idx]
			if (d && !updateRow(idx, d.key, d.valText))
				return false
		}
		if (Object.keys(rowErrors).length > 0)
			return false
		return tryCommitDraft(false)
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

		delegate: Column {
			id: kvRow
			required property var modelData
			required property int index

			readonly property string rowError: String(kvEditor.rowErrors[index] ?? "")
			readonly property var rowEnvVars: kvEditor.valueMode === "string"
				? kvEditor.extractEnvVars(valInput.text)
				: []
			readonly property var missingVars: rowEnvVars.filter(v =>
				Fields.has(kvEditor.envStatus, v) && kvEditor.envStatus[v] === false)
			readonly property bool hasEnvBadge: rowEnvVars.length > 0
				&& (rowEnvVars.every(v => Fields.has(kvEditor.envStatus, v)) || missingVars.length > 0)
			readonly property bool envOk: missingVars.length === 0 && hasEnvBadge
			readonly property int trailingWidth: kvEditor.allowReorder ? 84 : 28
			readonly property real keyBoxWidth: Math.max(100, Math.floor((width - trailingWidth - 12) * 0.38))

			width: kvEditor.width
			spacing: 4

			Item {
				width: parent.width
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
						onTextEdited: kvEditor.onRowEdited(kvRow.index, text, valInput.text)
						onEditingFinished: kvEditor.updateRow(kvRow.index, text, valInput.text)
					}
				}

				Rectangle {
					anchors { left: keyBox.right; leftMargin: 6; right: rowActions.left; rightMargin: 6; top: parent.top; bottom: parent.bottom }
					radius: Theme.radiusSmall + 2
					color: Theme.background
					border.color: kvRow.rowError !== ""
						? Theme.dangerText
						: (valInput.activeFocus ? Theme.accentText : Theme.border)

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
						text: kvEditor.formatEntryValue(kvRow.modelData)
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						clip: true
						onTextEdited: kvEditor.onRowEdited(kvRow.index, keyInput.text, text)
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

				Row {
					id: rowActions
					anchors { right: parent.right; verticalCenter: parent.verticalCenter }
					spacing: 2

					CircleButton {
						visible: kvEditor.allowReorder
						implicitWidth: 26
						implicitHeight: 26
						enabled: kvRow.index > 0
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Move entry up")
						onClicked: kvEditor.moveRow(kvRow.index, -1)

						Chevron {
							anchors.centerIn: parent
							rotation: -90
							color: Theme.textMuted
						}
					}

					CircleButton {
						visible: kvEditor.allowReorder
						implicitWidth: 26
						implicitHeight: 26
						enabled: kvRow.index + 1 < kvEditor.entries.length
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Move entry down")
						onClicked: kvEditor.moveRow(kvRow.index, 1)

						Chevron {
							anchors.centerIn: parent
							rotation: 90
							color: Theme.textMuted
						}
					}

					IconButton {
						id: deleteBtn
						buttonSize: 28
						iconName: "close"
						iconSize: 16
						Accessible.name: qsTr("Remove entry")
						onClicked: kvEditor.removeRow(kvRow.index)
					}
				}
			}

			Text {
				textFormat: Text.PlainText
				visible: kvRow.rowError !== ""
				width: parent.width
				text: kvRow.rowError
				color: Theme.dangerText
				font.pixelSize: Theme.smallFontSize
				wrapMode: Text.Wrap
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
			border.color: kvEditor.draftError !== "" && kvEditor.draftKeyText.trim() === ""
				? Theme.dangerText
				: (newKeyInput.activeFocus ? Theme.accentText : Theme.border)

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
				onTextEdited: kvEditor.updateDraftValidation()
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
			border.color: kvEditor.draftError !== "" && kvEditor.draftKeyText.trim() !== ""
				? Theme.dangerText
				: (newValInput.activeFocus ? Theme.accentText : Theme.border)

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
				onTextEdited: kvEditor.updateDraftValidation()
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
			enabled: kvEditor.enabled && kvEditor.draftKeyText.trim() !== ""
			onClicked: kvEditor.addDraft()
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: kvEditor.draftError !== ""
		width: parent.width
		text: kvEditor.draftError
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}
}
