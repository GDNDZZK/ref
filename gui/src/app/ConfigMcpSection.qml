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
			return
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
	}

	function liveStatusText(name) {
		const servers = Workspace.mcpServers ?? []
		for (const item of servers) {
			if (item.name === name)
				return String(item.status ?? "")
		}
		return ""
	}

	width: parent ? parent.width : 560
	spacing: 10

	Component.onCompleted: syncKeys()

	Connections {
		target: section.editor
		function onParsedChanged() { section.syncKeys() }
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
		model: section.serverKeys

		delegate: Rectangle {
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

			function cardMutate(fn) {
				cardMutating = true
				section.mutating = true
				fn()
				section.mutating = false
				cardMutating = false
			}

			function readCommandArray() {
				const raw = section.editor.valueAt(["mcp", serverName, "command"])
				if (Array.isArray(raw))
					return raw.map(x => String(x ?? ""))
				if (typeof raw === "string" && raw.trim() !== "")
					return raw.trim().split(/\s+/)
				return []
			}

			function syncCard() {
				renameInput.text = serverName
				renameInput.errorText = ""

				const t = section.editor.valueAt(["mcp", serverName, "timeout"])
				timeoutField.value = typeof t === "number" && Number.isFinite(t) ? t : undefined

				commandEditor.values = readCommandArray()

				const cwdVal = section.editor.valueAt(["mcp", serverName, "cwd"])
				cwdField.text = typeof cwdVal === "string" ? cwdVal : ""

				const envMap = section.editor.valueAt(["mcp", serverName, "environment"])
				const fallbackEnv = section.editor.valueAt(["mcp", serverName, "env"])
				const chosenEnv = (envMap && typeof envMap === "object" && !Array.isArray(envMap))
					? envMap
					: ((fallbackEnv && typeof fallbackEnv === "object" && !Array.isArray(fallbackEnv)) ? fallbackEnv : {})
				envEditor.loadFromMap(chosenEnv)

				const urlVal = section.editor.valueAt(["mcp", serverName, "url"])
				urlField.text = typeof urlVal === "string" ? urlVal : ""

				const hdrs = section.editor.valueAt(["mcp", serverName, "headers"])
				headersEditor.loadFromMap(hdrs && typeof hdrs === "object" && !Array.isArray(hdrs) ? hdrs : {})
			}

			function commitRename() {
				const target = renameInput.text.trim()
				if (target === "" || target === serverName) {
					renameInput.text = serverName
					renameInput.errorText = ""
					return
				}
				if (!section.editor.renameKeyAt(["mcp"], serverName, target)) {
					renameInput.errorText = qsTr("Server \"%1\" already exists.").arg(target)
					return
				}
				renameInput.errorText = ""
				section.setExpanded(target, true)
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
							width: parent.width - renameBtn.width - 6
							placeholderText: qsTr("Server name")
							onEdited: errorText = ""
							onAccepted: serverCard.commitRename()
						}

						DialogButton {
							id: renameBtn
							text: qsTr("Rename")
							enabled: renameInput.text.trim() !== "" && renameInput.text.trim() !== serverCard.serverName
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
					}

					NumberField {
						id: timeoutField
						width: parent.width
						minimum: 0
						step: 5000
						suffix: "ms"
						placeholderText: "30000"
						onValueModified: val => serverCard.cardMutate(() => section.editor.setValueAt(["mcp", serverCard.serverName, "timeout"], val))
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
							width: parent.width
							monospace: true
							allowCommandSplit: true
							placeholderText: qsTr("npx -y @modelcontextprotocol/server-github")
							emptyText: qsTr("No command configured")
							onValuesModified: vals => serverCard.cardMutate(() => section.editor.setStringListAt(["mcp", serverCard.serverName, "command"], vals, false))
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
							onEdited: serverCard.cardMutate(() => section.editor.setStringAt(["mcp", serverCard.serverName, "cwd"], text))
						}
					}

					FormRow {
						label: qsTr("Environment variables")
						badge: "environment"
						description: qsTr("Passed to the local MCP process. Supports {env:VAR} and {file:…}.")
						stacked: true
						showReset: { void section.editor.revision; return section.editor.contains(["mcp", serverCard.serverName, "environment"]) }
						onResetClicked: {
							section.forceActiveFocus()
							section.editor.removeAt(["mcp", serverCard.serverName, "environment"], false)
						}

						KeyValueEditor {
							id: envEditor
							width: parent.width
							valueMode: "string"
							envStatus: section.editor.envStatus
							keyPlaceholder: "GITHUB_TOKEN"
							valuePlaceholder: "{env:GITHUB_TOKEN}"
							emptyText: qsTr("No extra environment variables")
							onMapModified: newMap => serverCard.cardMutate(() => {
								if (Object.keys(newMap).length === 0)
									section.editor.removeAt(["mcp", serverCard.serverName, "environment"], false)
								else
									section.editor.setValueAt(["mcp", serverCard.serverName, "environment"], newMap)
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
						}

						KeyValueEditor {
							id: headersEditor
							width: parent.width
							valueMode: "string"
							envStatus: section.editor.envStatus
							keyPlaceholder: "Authorization"
							valuePlaceholder: "Bearer {env:MCP_TOKEN}"
							emptyText: qsTr("No custom HTTP headers")
							onMapModified: newMap => serverCard.cardMutate(() => {
								if (Object.keys(newMap).length === 0)
									section.editor.removeAt(["mcp", serverCard.serverName, "headers"], false)
								else
									section.editor.setValueAt(["mcp", serverCard.serverName, "headers"], newMap)
							})
						}
					}
				}
			}
		}
	}
}
