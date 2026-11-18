pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 3: Built-in agent overrides (`build`, `plan`, `general`, `explore`,
// `compaction`) and custom agents (`agent.<name>`).
Column {
	id: section
	objectName: "configAgentsSection"

	required property ConfigEditorController editor
	property bool mutating: false
	property var customAgentKeys: []
	property var modelOpts: []
	property var expandedMap: ({})

	readonly property var builtinAgents: [
		{ name: "build", summary: qsTr("Default agent with full tool access and session spawning.") },
		{ name: "plan", summary: qsTr("Read-only analysis and planning (edits denied, bash asks).") },
		{ name: "general", summary: qsTr("Multi-step tasks in an independent session.") },
		{ name: "explore", summary: qsTr("Fast, read-only codebase exploration.") },
		{ name: "compaction", summary: qsTr("Hidden summarizer used during context compaction.") }
	]
	readonly property var builtinNames: ["build", "plan", "general", "explore", "compaction"]

	function sameKeys(a, b) {
		if (a.length !== b.length)
			return false
		for (let i = 0; i < a.length; ++i) {
			if (a[i] !== b[i])
				return false
		}
		return true
	}

	function syncSection() {
		modelOpts = editor.modelOptions()
		const all = editor.keysAt(["agent"])
		const custom = all.filter(k => !builtinNames.includes(k))
		if (!sameKeys(customAgentKeys, custom))
			customAgentKeys = custom
	}

	function isExpanded(key) {
		return Boolean(expandedMap[key])
	}

	function setExpanded(key, open) {
		const next = Object.assign({}, expandedMap)
		next[key] = open
		expandedMap = next
	}

	function addCustomAgent() {
		const name = newAgentField.text.trim()
		if (name === "") {
			newAgentField.errorText = qsTr("Enter an agent name.")
			return
		}
		if (builtinNames.includes(name)) {
			newAgentField.errorText = qsTr("\"%1\" is a built-in agent — customize it above.").arg(name)
			return
		}
		if (editor.keysAt(["agent"]).includes(name)) {
			newAgentField.errorText = qsTr("Agent \"%1\" already exists.").arg(name)
			return
		}
		newAgentField.errorText = ""
		newAgentField.text = ""
		setExpanded("custom:" + name, true)
		editor.setValueAt(["agent", name], {
			description: "",
			prompt: ""
		})
	}

	width: parent ? parent.width : 560
	spacing: 10

	Component.onCompleted: syncSection()

	Connections {
		target: section.editor
		function onParsedChanged() { section.syncSection() }
	}

	Connections {
		target: Workspace.config
		function onChanged() { section.modelOpts = section.editor.modelOptions() }
	}

	component AgentCard: Rectangle {
		id: card
		required property string agentName
		required property bool isBuiltin
		property string builtinSummary: ""
		readonly property string expandKey: (isBuiltin ? "builtin:" : "custom:") + agentName
		readonly property bool expanded: section.isExpanded(expandKey)
		readonly property bool customized: {
			void section.editor.revision
			return section.editor.contains(["agent", agentName])
		}
		property bool cardMutating: false
		// "inherit" | "number" | "map"
		property string delayMode: "inherit"

		function cardMutate(fn) {
			cardMutating = true
			section.mutating = true
			fn()
			section.mutating = false
			cardMutating = false
		}

		function strField(key) {
			const v = section.editor.valueAt(["agent", agentName, key])
			return typeof v === "string" ? v : ""
		}

		function numField(key) {
			const v = section.editor.valueAt(["agent", agentName, key])
			return typeof v === "number" && Number.isFinite(v) ? v : undefined
		}

		function boolField(key, fallback) {
			const v = section.editor.valueAt(["agent", agentName, key])
			return typeof v === "boolean" ? v : fallback
		}

		function syncCard() {
			if (!isBuiltin) {
				renameInput.text = agentName
				renameInput.errorText = ""
			}
			descField.text = strField("description")
			modeSelect.value = strField("mode")
			modelSelect.value = strField("model")
			variantField.text = strField("variant")
			colorPicker.value = strField("color")
			tempField.value = numField("temperature")
			topPField.value = numField("top_p")
			promptField.text = strField("prompt")

			const opts = section.editor.valueAt(["agent", agentName, "options"])
			optionsEditor.loadFromMap(opts && typeof opts === "object" && !Array.isArray(opts) ? opts : {})

			agentPermEditor.loadFromValue(section.editor.valueAt(["agent", agentName, "permission"]))

			const rawDelay = section.editor.valueAt(["agent", agentName, "delay"])
			if (rawDelay === undefined || rawDelay === null) {
				delayMode = "inherit"
				agentDelayNumber.value = undefined
				agentDelayMap.loadFromMap({})
			} else if (typeof rawDelay === "number" && Number.isFinite(rawDelay)) {
				delayMode = "number"
				agentDelayNumber.value = rawDelay
				agentDelayMap.loadFromMap({})
			} else if (typeof rawDelay === "object" && !Array.isArray(rawDelay)) {
				delayMode = "map"
				agentDelayNumber.value = undefined
				agentDelayMap.loadFromMap(rawDelay)
			} else {
				delayMode = "inherit"
				agentDelayNumber.value = undefined
				agentDelayMap.loadFromMap({})
			}
		}

		function commitRename() {
			const target = renameInput.text.trim()
			if (target === "" || target === agentName) {
				renameInput.text = agentName
				renameInput.errorText = ""
				return
			}
			if (section.builtinNames.includes(target) || !section.editor.renameKeyAt(["agent"], agentName, target)) {
				renameInput.errorText = qsTr("Agent \"%1\" already exists.").arg(target)
				return
			}
			renameInput.errorText = ""
			section.setExpanded("custom:" + target, true)
		}

		function resetKey(key) {
			section.forceActiveFocus()
			section.editor.removeAt(["agent", agentName, key], isBuiltin)
		}

		width: section.width
		implicitHeight: headerRow.height + (expanded ? bodyCol.implicitHeight + 16 : 0)
		radius: Theme.radius
		color: Theme.background
		border.color: Theme.border
		clip: true

		Component.onCompleted: syncCard()

		Connections {
			target: section.editor
			function onParsedChanged() {
				if (!card.cardMutating)
					card.syncCard()
			}
		}

		Item {
			id: headerRow
			width: parent.width
			height: 46

			Item {
				anchors { left: parent.left; right: trailingActions.left; top: parent.top; bottom: parent.bottom }

				Chevron {
					id: chev
					x: 14
					anchors.verticalCenter: parent.verticalCenter
					rotation: card.expanded ? 90 : 0
					color: Theme.textMuted
					Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				Rectangle {
					id: dot
					anchors { left: chev.right; leftMargin: 10; verticalCenter: parent.verticalCenter }
					width: 10
					height: 10
					radius: 5
					color: {
						void section.editor.revision
						return Theme.agentColor(card.strField("color"), 0)
					}
				}

				Text {
					id: nameText
					textFormat: Text.PlainText
					anchors { left: dot.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					text: card.agentName
					color: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize; bold: true }
				}

				Rectangle {
					id: customBadge
					visible: card.isBuiltin && card.customized
					anchors { left: nameText.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					width: customBadgeText.implicitWidth + 12
					height: 20
					radius: 4
					color: Theme.userBubble
					border.color: Theme.userBubbleBorder

					Text {
						id: customBadgeText
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: qsTr("Customized")
						color: Theme.accentText
						font.pixelSize: Theme.microFontSize
					}
				}

				Text {
					textFormat: Text.PlainText
					anchors {
						left: customBadge.visible ? customBadge.right : nameText.right
						leftMargin: 10
						right: parent.right
						rightMargin: 8
						verticalCenter: parent.verticalCenter
					}
					text: {
						void section.editor.revision
						const d = card.strField("description")
						return d !== "" ? d : card.builtinSummary
					}
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: section.setExpanded(card.expandKey, !card.expanded) }
			}

			Row {
				id: trailingActions
				anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
				spacing: 6

				TextButton {
					visible: card.isBuiltin && card.customized
					anchors.verticalCenter: parent.verticalCenter
					text: qsTr("Reset override")
					onClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["agent", card.agentName])
					}
				}

				IconButton {
					visible: !card.isBuiltin
					anchors.verticalCenter: parent.verticalCenter
					buttonSize: 28
					iconName: "trash"
					iconSize: 16
					tooltipText: qsTr("Delete agent")
					Accessible.name: qsTr("Delete agent")
					onClicked: section.editor.removeAt(["agent", card.agentName])
				}
			}
		}

		Column {
			id: bodyCol
			visible: card.expanded
			y: headerRow.height
			x: 14
			width: parent.width - 28
			spacing: 4

			Rectangle { width: parent.width; height: 1; color: Theme.border }

			FormRow {
				visible: !card.isBuiltin
				label: qsTr("Agent name")
				showDivider: true

				Row {
					width: parent.width
					spacing: 6

					DialogField {
						id: renameInput
						width: parent.width - renameBtn.width - 6
						placeholderText: qsTr("Agent name")
						onEdited: errorText = ""
						onAccepted: card.commitRename()
					}

					DialogButton {
						id: renameBtn
						text: qsTr("Rename")
						enabled: renameInput.text.trim() !== "" && renameInput.text.trim() !== card.agentName
						onClicked: card.commitRename()
					}
				}
			}

			FormRow {
				label: qsTr("Description")
				badge: "description"
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "description"]) }
				onResetClicked: card.resetKey("description")

				DialogField {
					id: descField
					width: parent.width
					placeholderText: card.isBuiltin ? card.builtinSummary : qsTr("Shown in agent picker and spawn descriptions")
					onEdited: card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "description"], text))
				}
			}

			FormRow {
				label: qsTr("Default model")
				badge: "model"
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "model"]) }
				onResetClicked: card.resetKey("model")

				SelectField {
					id: modelSelect
					width: parent.width
					editable: true
					clearable: true
					options: section.modelOpts
					placeholderText: qsTr("Inherit top-level model")
					onActivated: val => card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "model"], val))
				}
			}

			FormRow {
				label: qsTr("Default variant")
				badge: "variant"
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "variant"]) }
				onResetClicked: card.resetKey("variant")

				DialogField {
					id: variantField
					width: parent.width
					placeholderText: qsTr("e.g. high")
					onEdited: card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "variant"], text))
				}
			}

			FormRow {
				label: qsTr("Mode")
				badge: "mode"
				description: qsTr("opencode compatibility mode (primary, subagent, all).")
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "mode"]) }
				onResetClicked: card.resetKey("mode")

				SelectField {
					id: modeSelect
					width: parent.width
					clearable: true
					options: [
						{ label: qsTr("Default"), value: "" },
						{ label: "primary", value: "primary" },
						{ label: "subagent", value: "subagent" },
						{ label: "all", value: "all" }
					]
					placeholderText: qsTr("Default")
					onActivated: val => card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "mode"], val))
				}
			}

			FormRow {
				label: qsTr("Badge color")
				badge: "color"
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "color"]) }
				onResetClicked: card.resetKey("color")

				ColorSwatchPicker {
					id: colorPicker
					onValueModified: val => card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "color"], val))
				}
			}

			FormRow {
				label: qsTr("Creatable by peers")
				badge: "creatable"
				description: qsTr("Allow other sessions to create sessions with this agent.")
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "creatable"]) }
				onResetClicked: card.resetKey("creatable")

				Switch {
					accessibleName: qsTr("Creatable by peers")
					checked: { void section.editor.revision; return card.boolField("creatable", card.agentName !== "compaction") }
					onToggled: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "creatable"], val))
				}
			}

			FormRow {
				label: qsTr("Can spawn sessions")
				badge: "can_spawn"
				description: qsTr("Grants create_session, resume_session, and close_session tools.")
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "can_spawn"]) }
				onResetClicked: card.resetKey("can_spawn")

				Switch {
					accessibleName: qsTr("Can spawn sessions")
					checked: { void section.editor.revision; return card.boolField("can_spawn", card.agentName === "build") }
					onToggled: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "can_spawn"], val))
				}
			}

			FormRow {
				label: qsTr("Hidden")
				badge: "hidden"
				description: qsTr("Hide from agent picker and peer session lists.")
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "hidden"]) }
				onResetClicked: card.resetKey("hidden")

				Switch {
					accessibleName: qsTr("Hidden")
					checked: { void section.editor.revision; return card.boolField("hidden", card.agentName === "compaction") }
					onToggled: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "hidden"], val))
				}
			}

			FormRow {
				label: qsTr("Disable agent")
				badge: "disable"
				description: qsTr("Remove this agent from runtime availability.")
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "disable"]) }
				onResetClicked: card.resetKey("disable")

				Switch {
					accessibleName: qsTr("Disable agent")
					checked: { void section.editor.revision; return card.boolField("disable", false) }
					onToggled: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "disable"], val))
				}
			}

			FormRow {
				label: qsTr("Temperature")
				badge: "temperature"
				controlWidth: 180
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "temperature"]) }
				onResetClicked: card.resetKey("temperature")

				NumberField {
					id: tempField
					width: parent.width
					decimals: true
					minimum: 0
					maximum: 2
					step: 0.1
					placeholderText: qsTr("Model default")
					onValueModified: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "temperature"], val))
				}
			}

			FormRow {
				label: qsTr("Top-p")
				badge: "top_p"
				controlWidth: 180
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "top_p"]) }
				onResetClicked: card.resetKey("top_p")

				NumberField {
					id: topPField
					width: parent.width
					decimals: true
					minimum: 0
					maximum: 1
					step: 0.05
					placeholderText: qsTr("Model default")
					onValueModified: val => card.cardMutate(() => section.editor.setValueAt(["agent", card.agentName, "top_p"], val))
				}
			}

			FormRow {
				label: qsTr("System prompt")
				badge: "prompt"
				description: qsTr("Placed at the start of the system prompt. Use {file:./prompts/name.md} for external files.")
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "prompt"]) }
				onResetClicked: card.resetKey("prompt")

				MultilineField {
					id: promptField
					width: parent.width
					minHeight: 100
					placeholderText: qsTr("Agent instructions or {file:./prompts/agent.md}")
					onEdited: card.cardMutate(() => section.editor.setStringAt(["agent", card.agentName, "prompt"], text))
				}
			}

			FormRow {
				label: qsTr("Request options")
				badge: "options"
				description: qsTr("Extra JSON request body fields merged over model options.")
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "options"]) }
				onResetClicked: card.resetKey("options")

				KeyValueEditor {
					id: optionsEditor
					width: parent.width
					valueMode: "json"
					keyPlaceholder: qsTr("Key (e.g. reasoningEffort)")
					valuePlaceholder: qsTr("JSON value (e.g. \"high\")")
					emptyText: qsTr("No extra request options")
					onMapModified: newMap => card.cardMutate(() => {
						if (Object.keys(newMap).length === 0)
							section.editor.removeAt(["agent", card.agentName, "options"], card.isBuiltin)
						else
							section.editor.setValueAt(["agent", card.agentName, "options"], newMap)
					})
				}
			}

			FormRow {
				label: qsTr("Permission overrides")
				badge: "permission"
				description: qsTr("Evaluated after global permissions (takes priority over global rules).")
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "permission"]) }
				onResetClicked: card.resetKey("permission")

				PermissionRulesEditor {
					id: agentPermEditor
					width: parent.width
					allowInherit: true
					onPermissionModified: newPerm => card.cardMutate(() => {
						if (newPerm === undefined || newPerm === null) {
							section.editor.removeAt(["agent", card.agentName, "permission"], card.isBuiltin)
							return
						}
						if (typeof newPerm === "object" && !Array.isArray(newPerm) && Object.keys(newPerm).length === 0) {
							section.editor.removeAt(["agent", card.agentName, "permission"], card.isBuiltin)
							return
						}
						section.editor.setValueAt(["agent", card.agentName, "permission"], newPerm)
					})
				}
			}

			FormRow {
				label: qsTr("Tool delay overrides")
				badge: "delay"
				description: qsTr("Checked before global delays.")
				stacked: true
				showReset: { void section.editor.revision; return section.editor.contains(["agent", card.agentName, "delay"]) }
				onResetClicked: card.resetKey("delay")

				Column {
					width: parent.width
					spacing: 8

					Segmented {
						width: parent.width
						model: [
							{ text: qsTr("Inherit"), value: "inherit" },
							{ text: qsTr("All tools"), value: "number" },
							{ text: qsTr("Per-tool"), value: "map" }
						]
						value: card.delayMode
						onActivated: m => {
							if (card.delayMode === m)
								return
							section.forceActiveFocus()
							card.delayMode = m
							card.cardMutate(() => {
								if (m === "inherit") {
									section.editor.removeAt(["agent", card.agentName, "delay"], card.isBuiltin)
								} else if (m === "number") {
									agentDelayNumber.value = 1
									section.editor.setValueAt(["agent", card.agentName, "delay"], 1)
								} else {
									const init = { "bash": 1 }
									agentDelayMap.loadFromMap(init)
									section.editor.setValueAt(["agent", card.agentName, "delay"], init)
								}
							})
						}
					}

					NumberField {
						id: agentDelayNumber
						visible: card.delayMode === "number"
						width: 200
						decimals: true
						minimum: 0
						step: 1
						suffix: "s"
						placeholderText: "1"
						onValueModified: val => card.cardMutate(() => {
							if (val === undefined)
								section.editor.removeAt(["agent", card.agentName, "delay"], card.isBuiltin)
							else
								section.editor.setValueAt(["agent", card.agentName, "delay"], val)
						})
					}

					KeyValueEditor {
						id: agentDelayMap
						visible: card.delayMode === "map"
						width: parent.width
						valueMode: "number"
						keyPlaceholder: qsTr("Tool glob (e.g. bash)")
						valuePlaceholder: qsTr("Seconds")
						onMapModified: newMap => card.cardMutate(() => {
							if (Object.keys(newMap).length === 0)
								section.editor.removeAt(["agent", card.agentName, "delay"], card.isBuiltin)
							else
								section.editor.setValueAt(["agent", card.agentName, "delay"], newMap)
						})
					}
				}
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Built-in Agents")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Override individual settings (model, variant, permissions, prompt) on built-in agents without replacing their defaults.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Repeater {
		model: section.builtinAgents

		delegate: AgentCard {
			required property var modelData
			agentName: modelData.name
			isBuiltin: true
			builtinSummary: modelData.summary
		}
	}

	Rectangle { width: parent.width; height: 1; color: Theme.border }

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 4
		text: qsTr("Custom Agents")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Item {
		width: parent.width
		implicitHeight: newAgentField.implicitHeight

		DialogField {
			id: newAgentField
			objectName: "newCustomAgentField"
			anchors { left: parent.left; right: addAgentBtn.left; rightMargin: 8; top: parent.top }
			placeholderText: qsTr("Custom agent name (e.g. reviewer, security)")
			onEdited: errorText = ""
			onAccepted: section.addCustomAgent()
		}

		DialogButton {
			id: addAgentBtn
			objectName: "addCustomAgentButton"
			anchors { right: parent.right; top: parent.top }
			kind: DialogButton.Primary
			text: qsTr("+ Add Custom Agent")
			onClicked: section.addCustomAgent()
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: section.customAgentKeys.length === 0
		width: parent.width
		text: qsTr("No custom agents defined in this file.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}

	Repeater {
		model: section.customAgentKeys

		delegate: AgentCard {
			required property string modelData
			agentName: modelData
			isBuiltin: false
		}
	}
}
