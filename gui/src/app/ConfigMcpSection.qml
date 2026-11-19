pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 5: MCP Servers (`mcp.<server>`: local stdio & remote Streamable HTTP).
Column {
	id: section
	objectName: "configMcpSection"

	required property ConfigEditorController editor
	property bool mutating: false
	property var serverKeys: []
	property var expandedMap: ({})
	property int cardsRevision: 0

	readonly property bool hasValidationError: {
		void cardsRevision
		void serverKeys
		if (newServerField.hasError)
			return true
		for (let i = 0; i < serverRepeater.count; ++i) {
			const c = serverRepeater.itemAt(i) as ServerCard
			if (c && c.hasValidationError)
				return true
		}
		return false
	}
	readonly property bool hasPendingChanges: {
		void cardsRevision
		void serverKeys
		if (newServerField.text.trim() !== "")
			return true
		for (let i = 0; i < serverRepeater.count; ++i) {
			const c = serverRepeater.itemAt(i) as ServerCard
			if (c && c.hasPendingChanges)
				return true
		}
		return false
	}

	function sameKeys(a, b) {
		if (a.length !== b.length)
			return false
		for (let i = 0; i < a.length; ++i) {
			if (a[i] !== b[i])
				return false
		}
		return true
	}

	function syncKeys() {
		const next = editor.keysAt(["mcp"])
		if (!sameKeys(serverKeys, next))
			serverKeys = next
	}

	function isExpanded(name) {
		return Boolean(expandedMap[name])
	}

	function setExpanded(name, open) {
		const next = Object.assign({}, expandedMap)
		next[name] = open
		expandedMap = next
	}

	function renameExpanded(oldName, newName) {
		const next = Object.assign({}, expandedMap)
		const wasOpen = Boolean(next[oldName])
		delete next[oldName]
		next[newName] = wasOpen
		expandedMap = next
	}

	function uniqueServerName(base) {
		const existing = editor.keysAt(["mcp"])
		if (!existing.includes(base))
			return base
		for (let i = 2; i < 1000; ++i) {
			const candidate = base + "-" + i
			if (!existing.includes(candidate))
				return candidate
		}
		return base
	}

	function addServer(kind) {
		let name = newServerField.text.trim()
		if (name === "")
			name = uniqueServerName(kind === "remote" ? "remote-mcp" : "local-mcp")
		if (editor.keysAt(["mcp"]).includes(name)) {
			newServerField.errorText = qsTr("MCP server \"%1\" already exists.").arg(name)
			return false
		}
		newServerField.errorText = ""
		newServerField.text = ""
		setExpanded(name, true)
		if (kind === "remote") {
			editor.setValueAt(["mcp", name], {
				type: "remote",
				url: "https://example.com/mcp"
			})
		} else {
			editor.setValueAt(["mcp", name], {
				type: "local",
				command: []
			})
		}
		return true
	}

	function liveStatusText(name) {
		const servers = Workspace.mcpServers ?? []
		for (const item of servers) {
			if (item.name === name)
				return String(item.status ?? "")
		}
		return ""
	}

	function commitPending() {
		if (newServerField.text.trim() !== "" && !addServer("local"))
			return false
		for (let i = 0; i < serverRepeater.count; ++i) {
			const c = serverRepeater.itemAt(i) as ServerCard
			if (c && !c.commitPending())
				return false
		}
		return !hasValidationError
	}

	function resetSubEditors() {
		newServerField.text = ""
		newServerField.errorText = ""
		for (let i = 0; i < serverRepeater.count; ++i) {
			const c = serverRepeater.itemAt(i) as ServerCard
			if (c)
				c.resetCardState()
		}
		syncKeys()
	}

	width: parent ? parent.width : 560
	spacing: 10

	Component.onCompleted: syncKeys()

	Connections {
		target: section.editor
		function onParsedChanged() { section.syncKeys() }
	}

	component ServerCard: Rectangle {
		id: serverCard
		required property string modelData
		readonly property string serverName: modelData
		readonly property bool expanded: section.isExpanded(serverName)
		property bool cardMutating: false

		readonly property string effectiveType: {
			void section.editor.revision
			const t = section.editor.valueAt(["mcp", serverName, "type"])
			if (typeof t === "string") {
				const lower = t.trim().toLowerCase()
				if (lower === "remote" || lower === "http" || lower === "sse" || lower === "streamable-http")
					return "remote"
				if (lower === "local" || lower === "stdio")
					return "local"
			}
			const url = section.editor.valueAt(["mcp", serverName, "url"])
			return typeof url === "string" && url !== "" ? "remote" : "local"
		}

		readonly property bool isEnabled: {
			void section.editor.revision
			const e = section.editor.valueAt(["mcp", serverName, "enabled"])
			return typeof e === "boolean" ? e : true
		}

		readonly property string pendingRenameTarget: renameInput.text.trim()
		readonly property bool hasValidationError: renameInput.hasError
			|| timeoutField.hasValidationError
			|| (effectiveType === "local" && envEditor.hasValidationError)
			|| (effectiveType === "remote" && headersEditor.hasValidationError)
		readonly property bool hasPendingChanges: (pendingRenameTarget !== "" && pendingRenameTarget !== serverName)
			|| timeoutField.hasUncommittedEdit
			|| (effectiveType === "local" && (commandEditor.hasUncommittedEdit || envEditor.hasUncommittedEdit))
			|| (effectiveType === "remote" && headersEditor.hasUncommittedEdit)

		function cardMutate(fn) {
			cardMutating = true
			section.mutating = true
			fn()
			section.mutating = false
			cardMutating = false
		}

		function readCommandArray() {
			const raw = section.editor.valueAt(["mcp", serverName, "command"])
			if (Fields.isArray(raw))
				return Fields.toArray(raw).map(x => String(x ?? ""))
			const words = (typeof raw === "string" && raw.trim() !== "")
				? commandEditor.splitCommandLine(raw.trim())
				: []
			const rawArgs = section.editor.valueAt(["mcp", serverName, "args"])
			const extraArgs = Fields.isArray(rawArgs) ? Fields.toArray(rawArgs).map(x => String(x ?? "")) : []
			return words.concat(extraArgs)
		}

		function syncCard() {
			renameInput.text = serverName
			renameInput.errorText = ""

			const t = section.editor.valueAt(["mcp", serverName, "timeout"])
			timeoutField.value = typeof t === "number" && Number.isFinite(t) ? t : undefined

			commandEditor.values = readCommandArray()

			const cwdVal = section.editor.valueAt(["mcp", serverName, "cwd"])
			cwdField.text = typeof cwdVal === "string" ? cwdVal : ""

			if (section.editor.contains(["mcp", serverName, "environment"]))
				envEditor.loadFromEntries(section.editor.orderedEntriesAt(["mcp", serverName, "environment"]))
			else if (section.editor.contains(["mcp", serverName, "env"]))
				envEditor.loadFromEntries(section.editor.orderedEntriesAt(["mcp", serverName, "env"]))
			else
				envEditor.loadFromEntries([])

			const urlVal = section.editor.valueAt(["mcp", serverName, "url"])
			urlField.text = typeof urlVal === "string" ? urlVal : ""

			headersEditor.loadFromEntries(section.editor.orderedEntriesAt(["mcp", serverName, "headers"]))
		}

		function validateRenameLive() {
			const target = renameInput.text.trim()
			if (target !== "" && target !== serverName && section.editor.keysAt(["mcp"]).includes(target))
				renameInput.errorText = qsTr("Server \"%1\" already exists.").arg(target)
			else
				renameInput.errorText = ""
		}

		function commitRename() {
			const target = renameInput.text.trim()
			if (target === "" || target === serverName) {
				renameInput.text = serverName
				renameInput.errorText = ""
				return true
			}
			if (section.editor.keysAt(["mcp"]).includes(target)) {
				renameInput.errorText = qsTr("Server \"%1\" already exists.").arg(target)
				return false
			}
			renameInput.errorText = ""
			section.renameExpanded(serverName, target)
			section.editor.renameKeyAt(["mcp"], serverName, target)
			return true
		}

		function commitPending() {
			if (effectiveType === "local") {
				if (!commandEditor.commitPending() || !envEditor.commitPending())
					return false
			} else {
				if (!headersEditor.commitPending())
					return false
			}
			if (hasValidationError)
				return false
			return commitRename()
		}

		function resetCardState() {
			timeoutField.resetState()
			commandEditor.resetState()
			envEditor.resetState()
			headersEditor.resetState()
			syncCard()
		}

		width: section.width
		implicitHeight: headerBar.height + (expanded ? bodyCol.implicitHeight + 16 : 0)
		radius: Theme.radius
		color: Theme.background
		border.color: Theme.border
		clip: true

		Component.onCompleted: syncCard()

		Connections {
			target: section.editor
			function onParsedChanged() {
				if (!serverCard.cardMutating)
					serverCard.syncCard()
			}
		}

		Item {
			id: headerBar
			width: parent.width
			height: 46

			Item {
				anchors { left: parent.left; right: headerTrailing.left; top: parent.top; bottom: parent.bottom }

				Chevron {
					id: chev
					x: 14
					anchors.verticalCenter: parent.verticalCenter
					rotation: serverCard.expanded ? 90 : 0
					color: Theme.textMuted
					Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				Text {
					id: srvTitle
					textFormat: Text.PlainText
					anchors { left: chev.right; leftMargin: 10; verticalCenter: parent.verticalCenter }
					text: serverCard.serverName
					color: serverCard.isEnabled ? Theme.text : Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize; bold: true }
				}

				Rectangle {
					id: typePill
					anchors { left: srvTitle.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					width: typePillText.implicitWidth + 12
					height: 20
					radius: 4
					color: Theme.codeBackground
					border.color: Theme.border

					Text {
						id: typePillText
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: serverCard.effectiveType === "remote" ? qsTr("Remote") : qsTr("Local")
						color: Theme.textMuted
						font.pixelSize: Theme.microFontSize
					}
				}

				Text {
					textFormat: Text.PlainText
					anchors { left: typePill.right; leftMargin: 8; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					text: {
						const st = section.liveStatusText(serverCard.serverName)
						return st !== "" ? ("· " + st) : ""
					}
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: section.setExpanded(serverCard.serverName, !serverCard.expanded) }
			}

			Row {
				id: headerTrailing
				anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
				spacing: 8

				Switch {
					anchors.verticalCenter: parent.verticalCenter
					accessibleName: qsTr("Enable %1").arg(serverCard.serverName)
					checked: serverCard.isEnabled
					onToggled: val => serverCard.cardMutate(() => section.editor.setValueAt(["mcp", serverCard.serverName, "enabled"], val))
				}

				IconButton {
					anchors.verticalCenter: parent.verticalCenter
					buttonSize: 28
					iconName: "trash"
					iconSize: 16
					tooltipText: qsTr("Delete MCP server")
					Accessible.name: qsTr("Delete MCP server")
					onClicked: section.editor.removeAt(["mcp", serverCard.serverName])
				}
			}
		}

		Column {
			id: bodyCol
			visible: serverCard.expanded
			y: headerBar.height
			x: 14
			width: parent.width - 28
			spacing: 4

			Rectangle { width: parent.width; height: 1; color: Theme.border }

			FormRow {
				label: qsTr("Server name")
				description: qsTr("Prefix used for exposed MCP tools (<server>_<tool>).")
				showDivider: true

				Row {
					width: parent.width
					spacing: 6

					DialogField {
						id: renameInput
						objectName: "mcpRenameField_" + serverCard.serverName
						width: parent.width - renameBtn.width - 6
						placeholderText: qsTr("Server name")
						onEdited: serverCard.validateRenameLive()
						onAccepted: serverCard.commitRename()
						onEditingFinished: serverCard.commitRename()
					}

					DialogButton {
						id: renameBtn
						text: qsTr("Rename")
						enabled: serverCard.pendingRenameTarget !== ""
							&& serverCard.pendingRenameTarget !== serverCard.serverName
							&& !renameInput.hasError
						onClicked: serverCard.commitRename()
					}
				}
			}

			FormRow {
				label: qsTr("Transport")
				badge: "type"
				showDivider: true

				Segmented {
					width: parent.width
					model: [
						{ text: qsTr("Local (stdio)"), value: "local" },
						{ text: qsTr("Remote (HTTP)"), value: "remote" }
					]
					value: serverCard.effectiveType
					onActivated: nextType => serverCard.cardMutate(() => section.editor.setValueAt(["mcp", serverCard.serverName, "type"], nextType))
				}
			}

			FormRow {
				label: qsTr("Enabled")
				badge: "enabled"
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["mcp", serverCard.serverName, "enabled"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["mcp", serverCard.serverName, "enabled"], false)
				}

				Switch {
					accessibleName: qsTr("Enabled")
					checked: serverCard.isEnabled
					onToggled: val => serverCard.cardMutate(() => section.editor.setValueAt(["mcp", serverCard.serverName, "enabled"], val))
				}
			}

			FormRow {
				label: qsTr("Startup timeout")
				badge: "timeout"
				description: qsTr("Milliseconds allowed for starting and listing tools (default 30000).")
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["mcp", serverCard.serverName, "timeout"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["mcp", serverCard.serverName, "timeout"], false)
					timeoutField.resetState()
				}

				NumberField {
					id: timeoutField
					objectName: "mcpTimeoutField_" + serverCard.serverName
					width: parent.width
					minimum: 1
					step: 5000
					suffix: "ms"
					placeholderText: "30000"
					onValueModified: val => serverCard.cardMutate(() => {
						if (val === undefined)
							section.editor.removeAt(["mcp", serverCard.serverName, "timeout"], false)
						else
							section.editor.setValueAt(["mcp", serverCard.serverName, "timeout"], val)
					})
				}
			}

			Column {
				visible: serverCard.effectiveType === "local"
				width: parent.width
				spacing: 4

				FormRow {
					label: qsTr("Command & arguments")
					badge: "command"
					description: qsTr("Executable followed by arguments. Paste a full command and click 'Split command' to tokenize.")
					stacked: true
					showDivider: true

					StringListEditor {
						id: commandEditor
						objectName: "mcpCommandEditor_" + serverCard.serverName
						width: parent.width
						monospace: true
						allowCommandSplit: true
						placeholderText: qsTr("npx -y @modelcontextprotocol/server-github")
						emptyText: qsTr("No command configured")
						onValuesModified: vals => serverCard.cardMutate(() => {
							section.editor.removeAt(["mcp", serverCard.serverName, "args"], false)
							section.editor.setStringListAt(["mcp", serverCard.serverName, "command"], vals, false)
						})
					}
				}

				FormRow {
					label: qsTr("Working directory")
					badge: "cwd"
					showDivider: true
					showReset: { void section.editor.revision; return section.editor.contains(["mcp", serverCard.serverName, "cwd"]) }
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["mcp", serverCard.serverName, "cwd"], false)
					}

					DialogField {
						id: cwdField
						width: parent.width
						placeholderText: qsTr("Session directory (default)")
						onEdited: serverCard.cardMutate(() => {
							if (text.trim() === "")
								section.editor.removeAt(["mcp", serverCard.serverName, "cwd"], false)
							else
								section.editor.setValueAt(["mcp", serverCard.serverName, "cwd"], text)
						})
						onEditingFinished: {
							const trimmed = text.trim()
							if (text !== trimmed)
								text = trimmed
							serverCard.cardMutate(() => {
								if (trimmed === "")
									section.editor.removeAt(["mcp", serverCard.serverName, "cwd"], false)
								else
									section.editor.setValueAt(["mcp", serverCard.serverName, "cwd"], trimmed)
							})
						}
					}
				}

				FormRow {
					label: qsTr("Environment variables")
					badge: "environment"
					description: qsTr("Passed to the local MCP process. Supports {env:VAR} and {file:…}.")
					stacked: true
					showReset: {
						void section.editor.revision
						return section.editor.contains(["mcp", serverCard.serverName, "environment"])
							|| section.editor.contains(["mcp", serverCard.serverName, "env"])
					}
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["mcp", serverCard.serverName, "environment"], false)
						section.editor.removeAt(["mcp", serverCard.serverName, "env"], false)
						envEditor.resetState()
					}

					KeyValueEditor {
						id: envEditor
						objectName: "mcpEnvEditor_" + serverCard.serverName
						width: parent.width
						valueMode: "string"
						envStatus: section.editor.envStatus
						keyPlaceholder: "GITHUB_TOKEN"
						valuePlaceholder: "{env:GITHUB_TOKEN}"
						emptyText: qsTr("No extra environment variables")
						onEntriesModified: nextEntries => serverCard.cardMutate(() => {
							section.editor.removeAt(["mcp", serverCard.serverName, "env"], false)
							section.editor.setOrderedObjectAt(["mcp", serverCard.serverName, "environment"], nextEntries, true, false)
						})
					}
				}
			}

			Column {
				visible: serverCard.effectiveType === "remote"
				width: parent.width
				spacing: 4

				FormRow {
					label: qsTr("Endpoint URL")
					badge: "url"
					showDivider: true

					DialogField {
						id: urlField
						width: parent.width
						placeholderText: "https://mcp.example.com/mcp"
						onEdited: serverCard.cardMutate(() => section.editor.setStringAt(["mcp", serverCard.serverName, "url"], text, false))
						onEditingFinished: {
							const trimmed = text.trim()
							if (text !== trimmed)
								text = trimmed
							serverCard.cardMutate(() => section.editor.setStringAt(["mcp", serverCard.serverName, "url"], trimmed, false))
						}
					}
				}

				FormRow {
					label: qsTr("HTTP headers")
					badge: "headers"
					description: qsTr("Extra headers sent with Streamable HTTP requests. Supports {env:VAR}.")
					stacked: true
					showReset: { void section.editor.revision; return section.editor.contains(["mcp", serverCard.serverName, "headers"]) }
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["mcp", serverCard.serverName, "headers"], false)
						headersEditor.resetState()
					}

					KeyValueEditor {
						id: headersEditor
						objectName: "mcpHeadersEditor_" + serverCard.serverName
						width: parent.width
						valueMode: "string"
						envStatus: section.editor.envStatus
						keyPlaceholder: "Authorization"
						valuePlaceholder: "Bearer {env:MCP_TOKEN}"
						emptyText: qsTr("No custom HTTP headers")
						onEntriesModified: nextEntries => serverCard.cardMutate(() => {
							section.editor.setOrderedObjectAt(["mcp", serverCard.serverName, "headers"], nextEntries, true, false)
						})
					}
				}
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("MCP Servers")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Configure Model Context Protocol servers over local stdio processes or remote Streamable HTTP endpoints.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Item {
		width: parent.width
		implicitHeight: newServerField.implicitHeight

		DialogField {
			id: newServerField
			objectName: "newMcpServerField"
			anchors { left: parent.left; right: addBtns.left; rightMargin: 8; top: parent.top }
			placeholderText: qsTr("Server name (e.g. github, docs)")
			onEdited: errorText = ""
			onAccepted: section.addServer("local")
		}

		Row {
			id: addBtns
			anchors { right: parent.right; top: parent.top }
			spacing: 6

			DialogButton {
				objectName: "addLocalMcpButton"
				kind: DialogButton.Primary
				text: qsTr("+ Add Local")
				onClicked: section.addServer("local")
			}

			DialogButton {
				objectName: "addRemoteMcpButton"
				text: qsTr("+ Add Remote")
				onClicked: section.addServer("remote")
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: section.serverKeys.length === 0
		width: parent.width
		topPadding: 4
		text: qsTr("No MCP servers configured in this file.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}

	Repeater {
		id: serverRepeater
		model: section.serverKeys
		onItemAdded: section.cardsRevision++
		onItemRemoved: section.cardsRevision++

		delegate: ServerCard {}
	}
}
