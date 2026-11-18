pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Editor for global or per-agent `permission`: supports global single-action modes
// (`"allow"` | `"ask"` | `"deny"`), optional `"inherit"` (unset), and an ordered rule table
// (`[Tool glob] [Pattern] [Allow|Ask|Deny] [↑ ↓ ×]`).
Column {
	id: rulesEditor

	property bool allowInherit: false
	// `"inherit"`, `"rules"`, `"allow"`, `"ask"`, or `"deny"`.
	property string mode: allowInherit ? "inherit" : "rules"
	// Array of `{ tool: string, pattern: string, action: "allow" | "ask" | "deny" }`.
	property var rules: []
	property var rowDrafts: ({})

	signal orderedPermissionModified(var orderedPermission)
	signal permissionModified(var newPermission)

	readonly property var modeTabs: {
		const base = [
			{ text: qsTr("Rule table"), value: "rules" },
			{ text: qsTr("Allow all"), value: "allow" },
			{ text: qsTr("Ask all"), value: "ask" },
			{ text: qsTr("Deny all"), value: "deny" }
		]
		return allowInherit ? [{ text: qsTr("Inherit"), value: "inherit" }].concat(base) : base
	}
	readonly property var actionTabs: [
		{ text: qsTr("Allow"), value: "allow" },
		{ text: qsTr("Ask"), value: "ask" },
		{ text: qsTr("Deny"), value: "deny" }
	]
	readonly property bool hasEmptyToolRule: mode === "rules"
		&& rules.some((r, idx) => effectiveRowTool(idx, r).trim() === "")
	readonly property bool hasValidationError: hasEmptyToolRule
	readonly property bool hasUncommittedEdit: Object.keys(rowDrafts).length > 0

	function normalizeAction(raw) {
		if (raw === false || raw === "deny" || raw === "reject" || raw === "no")
			return "deny"
		if (raw === "ask" || raw === "confirm" || raw === "prompt")
			return "ask"
		return "allow"
	}

	function effectiveRowTool(index, ruleObj) {
		const draft = rowDrafts[index]
		if (draft && draft.tool !== undefined)
			return String(draft.tool)
		return String(ruleObj.tool ?? "")
	}

	function appendNestedObjectRules(toolKey, entryObj, out) {
		for (const patKey of Object.keys(entryObj)) {
			if (!Fields.has(entryObj, patKey))
				continue
			out.push({
				tool: toolKey,
				pattern: patKey,
				action: normalizeAction(entryObj[patKey])
			})
		}
	}

	function loadRulesFromObject(obj) {
		const loaded = []
		for (const toolKey of Object.keys(obj)) {
			if (!Fields.has(obj, toolKey))
				continue
			const entry = obj[toolKey]
			if (entry !== null && typeof entry === "object" && !Fields.isArray(entry))
				appendNestedObjectRules(toolKey, entry, loaded)
			else
				loaded.push({ tool: toolKey, pattern: "", action: normalizeAction(entry) })
		}
		return loaded
	}

	function loadFromValue(val) {
		rowDrafts = ({})
		if (val === undefined || val === null) {
			mode = allowInherit ? "inherit" : "rules"
			rules = []
			return
		}
		if (typeof val === "string" || typeof val === "boolean") {
			mode = normalizeAction(val)
			rules = []
			return
		}
		if (Fields.isArray(val)) {
			mode = "rules"
			rules = Fields.toArray(val).map(item => ({
				tool: String(item.tool ?? ""),
				pattern: String(item.pattern ?? ""),
				action: normalizeAction(item.action)
			}))
			return
		}
		if (typeof val === "object") {
			mode = "rules"
			rules = loadRulesFromObject(val)
		}
	}

	function resetState() {
		rowDrafts = ({})
	}

	function toOrderedValue() {
		if (mode === "inherit")
			return undefined
		if (mode === "allow" || mode === "ask" || mode === "deny")
			return mode
		return rules.map(r => ({
			tool: String(r.tool ?? "").trim(),
			pattern: String(r.pattern ?? "").trim(),
			action: normalizeAction(r.action)
		}))
	}

	function toValue() {
		if (mode === "inherit")
			return undefined
		if (mode === "allow" || mode === "ask" || mode === "deny")
			return mode
		const out = {}
		for (const r of rules) {
			const tool = String(r.tool ?? "").trim()
			if (tool === "")
				continue
			const pattern = String(r.pattern ?? "").trim()
			const action = normalizeAction(r.action)
			if (pattern === "" && !Fields.has(out, tool)) {
				out[tool] = action
				continue
			}
			const pat = pattern === "" ? "*" : pattern
			if (!Fields.has(out, tool) || typeof out[tool] !== "object") {
				const previous = Fields.has(out, tool) ? { "*": out[tool] } : {}
				out[tool] = previous
			}
			out[tool][pat] = action
		}
		return out
	}

	function emitCurrent() {
		if (hasValidationError)
			return
		orderedPermissionModified(toOrderedValue())
		permissionModified(toValue())
	}

	function setRowDraft(index, patch) {
		const next = Object.assign({}, rowDrafts)
		next[index] = Object.assign({}, next[index] ?? {}, patch)
		rowDrafts = next
	}

	function clearRowDraftField(index, fieldName) {
		if (!Fields.has(rowDrafts, index))
			return
		const entry = Object.assign({}, rowDrafts[index])
		delete entry[fieldName]
		const next = Object.assign({}, rowDrafts)
		if (Object.keys(entry).length === 0)
			delete next[index]
		else
			next[index] = entry
		rowDrafts = next
	}

	function updateRule(index, patch) {
		if (index < 0 || index >= rules.length)
			return
		for (const k of Object.keys(patch))
			clearRowDraftField(index, k)
		const next = rules.slice()
		next[index] = Object.assign({}, next[index], patch)
		rules = next
		emitCurrent()
	}

	function commitPending() {
		const indices = Object.keys(rowDrafts).map(k => Number(k))
		if (indices.length > 0) {
			const next = rules.slice()
			for (const idx of indices) {
				if (idx < 0 || idx >= next.length)
					continue
				const d = rowDrafts[idx]
				const patch = {}
				if (d.tool !== undefined)
					patch.tool = String(d.tool).trim()
				if (d.pattern !== undefined)
					patch.pattern = String(d.pattern).trim()
				next[idx] = Object.assign({}, next[idx], patch)
			}
			rowDrafts = ({})
			rules = next
			if (!hasValidationError)
				emitCurrent()
		}
		return !hasValidationError
	}

	function moveRule(index, delta) {
		const target = index + delta
		if (index < 0 || index >= rules.length || target < 0 || target >= rules.length)
			return
		rowDrafts = ({})
		const next = rules.slice()
		const moved = next[index]
		next[index] = next[target]
		next[target] = moved
		rules = next
		emitCurrent()
	}

	function removeRule(index) {
		if (index < 0 || index >= rules.length)
			return
		rowDrafts = ({})
		const next = rules.slice()
		next.splice(index, 1)
		rules = next
		emitCurrent()
	}

	function addRule() {
		const next = rules.concat([{ tool: "bash", pattern: "*", action: "ask" }])
		rules = next
		emitCurrent()
	}

	spacing: 10
	width: parent ? parent.width : 560
	opacity: enabled ? 1 : Theme.disabledOpacity

	Segmented {
		width: parent.width
		model: rulesEditor.modeTabs
		value: rulesEditor.mode
		onActivated: newMode => {
			if (rulesEditor.mode === newMode)
				return
			rulesEditor.rowDrafts = ({})
			rulesEditor.mode = newMode
			rulesEditor.emitCurrent()
		}
	}

	Column {
		visible: rulesEditor.mode === "rules"
		width: parent.width
		spacing: 6

		Text {
			textFormat: Text.PlainText
			visible: rulesEditor.rules.length === 0
			width: parent.width
			text: qsTr("No permission rules configured (all tools allowed by default).")
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			wrapMode: Text.Wrap
		}

		Repeater {
			model: rulesEditor.rules

			delegate: Item {
				id: ruleRow
				required property var modelData
				required property int index

				readonly property bool emptyTool: toolInput.text.trim() === ""
				readonly property int actionWidth: 174
				readonly property int controlsWidth: 84
				readonly property real fieldsSpace: Math.max(140, width - actionWidth - controlsWidth - 18)
				readonly property real toolWidth: Math.floor(fieldsSpace * 0.42)

				width: rulesEditor.width
				height: 34

				Rectangle {
					id: toolBox
					width: ruleRow.toolWidth
					height: parent.height
					radius: Theme.radiusSmall + 2
					color: Theme.background
					border.color: ruleRow.emptyTool
						? Theme.dangerText
						: (toolInput.activeFocus ? Theme.accentText : Theme.border)

					TextInput {
						id: toolInput
						anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
						verticalAlignment: TextInput.AlignVCenter
						text: String(ruleRow.modelData.tool ?? "")
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						clip: true
						Accessible.name: qsTr("Tool glob")
						onTextEdited: rulesEditor.setRowDraft(ruleRow.index, { tool: text })
						onEditingFinished: rulesEditor.updateRule(ruleRow.index, { tool: text.trim() })

						PlaceholderText {
							anchors.fill: parent
							textInput: toolInput
							text: qsTr("Tool glob (e.g. bash)")
							font.pixelSize: Theme.smallFontSize
							elide: Text.ElideRight
						}
					}
				}

				Rectangle {
					id: patternBox
					anchors {
						left: toolBox.right
						leftMargin: 6
						right: actionSeg.left
						rightMargin: 6
						top: parent.top
						bottom: parent.bottom
					}
					radius: Theme.radiusSmall + 2
					color: Theme.background
					border.color: patternInput.activeFocus ? Theme.accentText : Theme.border

					TextInput {
						id: patternInput
						anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
						verticalAlignment: TextInput.AlignVCenter
						text: String(ruleRow.modelData.pattern ?? "")
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						clip: true
						Accessible.name: qsTr("Pattern")
						onTextEdited: rulesEditor.setRowDraft(ruleRow.index, { pattern: text })
						onEditingFinished: rulesEditor.updateRule(ruleRow.index, { pattern: text.trim() })

						PlaceholderText {
							anchors.fill: parent
							textInput: patternInput
							text: qsTr("* (all arguments)")
							font.pixelSize: Theme.smallFontSize
							elide: Text.ElideRight
						}
					}
				}

				Segmented {
					id: actionSeg
					anchors { right: rowButtons.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
					width: ruleRow.actionWidth
					height: 30
					model: rulesEditor.actionTabs
					value: String(ruleRow.modelData.action ?? "allow")
					onActivated: act => rulesEditor.updateRule(ruleRow.index, { action: act })
				}

				Row {
					id: rowButtons
					anchors { right: parent.right; verticalCenter: parent.verticalCenter }
					spacing: 2

					CircleButton {
						implicitWidth: 26
						implicitHeight: 26
						enabled: ruleRow.index > 0
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Move rule up")
						onClicked: rulesEditor.moveRule(ruleRow.index, -1)

						Chevron {
							anchors.centerIn: parent
							rotation: -90
							color: Theme.textMuted
						}
					}

					CircleButton {
						implicitWidth: 26
						implicitHeight: 26
						enabled: ruleRow.index + 1 < rulesEditor.rules.length
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Move rule down")
						onClicked: rulesEditor.moveRule(ruleRow.index, 1)

						Chevron {
							anchors.centerIn: parent
							rotation: 90
							color: Theme.textMuted
						}
					}

					IconButton {
						buttonSize: 28
						iconName: "close"
						iconSize: 16
						Accessible.name: qsTr("Remove rule")
						onClicked: rulesEditor.removeRule(ruleRow.index)
					}
				}
			}
		}

		Text {
			textFormat: Text.PlainText
			visible: rulesEditor.hasEmptyToolRule
			width: parent.width
			text: qsTr("Tool glob cannot be empty. Enter a tool name (e.g. bash or *) or remove the rule.")
			color: Theme.dangerText
			font.pixelSize: Theme.smallFontSize
			wrapMode: Text.Wrap
		}

		Row {
			spacing: 6

			DialogButton {
				text: qsTr("+ Add rule")
				onClicked: rulesEditor.addRule()
			}
		}
	}
}
