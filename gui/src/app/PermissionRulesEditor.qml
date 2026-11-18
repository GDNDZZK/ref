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

	function normalizeAction(raw) {
		if (raw === false || raw === "deny" || raw === "reject" || raw === "no")
			return "deny"
		if (raw === "ask" || raw === "confirm" || raw === "prompt")
			return "ask"
		return "allow"
	}

	function loadFromValue(val) {
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
		if (typeof val === "object") {
			mode = "rules"
			const loaded = []
			for (const toolKey of Object.keys(val)) {
				if (!Fields.has(val, toolKey))
					continue
				const entry = val[toolKey]
				if (entry !== null && typeof entry === "object" && !Array.isArray(entry)) {
					for (const patKey of Object.keys(entry)) {
						if (Fields.has(entry, patKey)) {
							loaded.push({
								tool: toolKey,
								pattern: patKey,
								action: normalizeAction(entry[patKey])
							})
						}
					}
				} else {
					loaded.push({
						tool: toolKey,
						pattern: "",
						action: normalizeAction(entry)
					})
				}
			}
			rules = loaded
		}
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
			} else {
				const pat = pattern === "" ? "*" : pattern
				if (!Fields.has(out, tool) || typeof out[tool] !== "object") {
					const previous = Fields.has(out, tool) ? { "*": out[tool] } : {}
					out[tool] = previous
				}
				out[tool][pat] = action
			}
		}
		return out
	}

	function emitCurrent() {
		permissionModified(toValue())
	}

	function updateRule(index, patch) {
		if (index < 0 || index >= rules.length)
			return
		const next = rules.slice()
		next[index] = Object.assign({}, next[index], patch)
		rules = next
		emitCurrent()
	}

	function moveRule(index, delta) {
		const target = index + delta
		if (index < 0 || index >= rules.length || target < 0 || target >= rules.length)
			return
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
					border.color: toolInput.activeFocus ? Theme.accentText : Theme.border

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

		Row {
			spacing: 6

			DialogButton {
				text: qsTr("+ Add rule")
				onClicked: rulesEditor.addRule()
			}
		}
	}
}
