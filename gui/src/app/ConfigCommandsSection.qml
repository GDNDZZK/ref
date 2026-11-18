pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 6: Custom slash commands (`command`), `instructions[]`, and `skills` (`string[]` or `{paths: string[]}`).
Column {
	id: section
	objectName: "configCommandsSection"

	required property ConfigEditorController editor
	property bool mutating: false
	property var commandKeys: []
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

	function mutate(fn) {
		mutating = true
		fn()
		mutating = false
	}

	function listAt(path) {
		const v = editor.valueAt(path)
		if (!Array.isArray(v))
			return []
		return v.map(item => String(item ?? ""))
	}

	function readSkillsList() {
		const raw = editor.valueAt(["skills"])
		if (Array.isArray(raw))
			return raw.map(item => String(item ?? ""))
		if (raw !== null && typeof raw === "object" && Array.isArray(raw.paths))
			return raw.paths.map(item => String(item ?? ""))
		return []
	}

	function writeSkillsList(vals) {
		mutate(() => {
			const raw = editor.valueAt(["skills"])
			if (raw !== null && typeof raw === "object" && !Array.isArray(raw)) {
				if (vals.length === 0) {
					const extraKeys = Object.keys(raw).filter(k => k !== "paths")
					if (extraKeys.length === 0)
						editor.removeAt(["skills"])
					else
						editor.removeAt(["skills", "paths"], false)
				} else {
					editor.setStringListAt(["skills", "paths"], vals, false)
				}
				return
			}
			editor.setStringListAt(["skills"], vals, true)
		})
	}

	function syncControls() {
		const nextKeys = editor.keysAt(["command"])
		if (!sameKeys(commandKeys, nextKeys))
			commandKeys = nextKeys
		instructionsEditor.values = listAt(["instructions"])
		skillsEditor.values = readSkillsList()
	}

	function isExpanded(name) {
		return Boolean(expandedMap[name])
	}

	function setExpanded(name, open) {
		const next = Object.assign({}, expandedMap)
		next[name] = open
		expandedMap = next
	}

	function addCommand() {
		let rawName = newCommandField.text.trim()
		if (rawName.startsWith("/"))
			rawName = rawName.slice(1).trim()
		if (rawName === "") {
			newCommandField.errorText = qsTr("Enter a command name.")
			return
		}
		if (editor.keysAt(["command"]).includes(rawName)) {
			newCommandField.errorText = qsTr("Command /%1 already exists.").arg(rawName)
			return
		}
		newCommandField.errorText = ""
		newCommandField.text = ""
		setExpanded(rawName, true)
		editor.setValueAt(["command", rawName], {
			description: "",
			template: "$ARGUMENTS"
		})
	}

	width: parent ? parent.width : 560
	spacing: 10

	Component.onCompleted: syncControls()

	Connections {
		target: section.editor
		function onParsedChanged() {
			if (!section.mutating)
				section.syncControls()
			else {
				const nextKeys = section.editor.keysAt(["command"])
				if (!section.sameKeys(section.commandKeys, nextKeys))
					section.commandKeys = nextKeys
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Custom Slash Commands")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Prompt templates invoked as /name. Supports $ARGUMENTS, $1–$9 positional words, and !`shell command` inline output.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Item {
		width: parent.width
		implicitHeight: newCommandField.implicitHeight

		DialogField {
			id: newCommandField
			objectName: "newCustomCommandField"
			anchors { left: parent.left; right: addCmdBtn.left; rightMargin: 8; top: parent.top }
			placeholderText: qsTr("Command name (e.g. review, fix-issue)")
			onEdited: errorText = ""
			onAccepted: section.addCommand()
		}

		DialogButton {
			id: addCmdBtn
			objectName: "addCustomCommandButton"
			anchors { right: parent.right; top: parent.top }
			kind: DialogButton.Primary
			text: qsTr("+ Add Command")
			onClicked: section.addCommand()
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: section.commandKeys.length === 0
		width: parent.width
		topPadding: 4
		bottomPadding: 4
		text: qsTr("No custom slash commands configured in this file.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}

	Repeater {
		model: section.commandKeys

		delegate: Rectangle {
			id: cmdCard
			required property string modelData
			readonly property string cmdName: modelData
			readonly property bool expanded: section.isExpanded(cmdName)
			property bool cardMutating: false

			function cardMutate(fn) {
				cardMutating = true
				section.mutating = true
				fn()
				section.mutating = false
				cardMutating = false
			}

			function syncCard() {
				renameInput.text = cmdName
				renameInput.errorText = ""
				const desc = section.editor.valueAt(["command", cmdName, "description"])
				descField.text = typeof desc === "string" ? desc : ""
				const tmpl = section.editor.valueAt(["command", cmdName, "template"])
				templateField.text = typeof tmpl === "string" ? tmpl : ""
			}

			function commitRename() {
				let target = renameInput.text.trim()
				if (target.startsWith("/"))
					target = target.slice(1).trim()
				if (target === "" || target === cmdName) {
					renameInput.text = cmdName
					renameInput.errorText = ""
					return
				}
				if (!section.editor.renameKeyAt(["command"], cmdName, target)) {
					renameInput.errorText = qsTr("Command /%1 already exists.").arg(target)
					return
				}
				renameInput.errorText = ""
				section.setExpanded(target, true)
			}

			width: section.width
			implicitHeight: cardHeader.height + (expanded ? cardBody.implicitHeight + 16 : 0)
			radius: Theme.radius
			color: Theme.background
			border.color: Theme.border
			clip: true

			Component.onCompleted: syncCard()

			Connections {
				target: section.editor
				function onParsedChanged() {
					if (!cmdCard.cardMutating)
						cmdCard.syncCard()
				}
			}

			Item {
				id: cardHeader
				width: parent.width
				height: 44

				Item {
					anchors { left: parent.left; right: deleteCmdBtn.left; top: parent.top; bottom: parent.bottom }

					Chevron {
						id: chev
						x: 14
						anchors.verticalCenter: parent.verticalCenter
						rotation: cmdCard.expanded ? 90 : 0
						color: Theme.textMuted
						Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					}

					Text {
						id: cmdTitle
						textFormat: Text.PlainText
						anchors { left: chev.right; leftMargin: 10; verticalCenter: parent.verticalCenter }
						text: "/" + cmdCard.cmdName
						color: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize; bold: true }
					}

					Text {
						textFormat: Text.PlainText
						anchors { left: cmdTitle.right; leftMargin: 10; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
						text: {
							void section.editor.revision
							const d = section.editor.valueAt(["command", cmdCard.cmdName, "description"])
							return typeof d === "string" ? d : ""
						}
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
						elide: Text.ElideRight
					}

					HoverHandler { cursorShape: Qt.PointingHandCursor }
					TapHandler { onTapped: section.setExpanded(cmdCard.cmdName, !cmdCard.expanded) }
				}
				IconButton {
					id: deleteCmdBtn
					anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					buttonSize: 28
					iconName: "trash"
					iconSize: 16
					tooltipText: qsTr("Delete command")
					Accessible.name: qsTr("Delete command")
					onClicked: section.editor.removeAt(["command", cmdCard.cmdName])
				}
			}

			Column {
				id: cardBody
				visible: cmdCard.expanded
				y: cardHeader.height
				x: 14
				width: parent.width - 28
				spacing: 4

				Rectangle { width: parent.width; height: 1; color: Theme.border }

				FormRow {
					label: qsTr("Command name")
					description: qsTr("Slash name typed after '/'.")
					showDivider: true

					Row {
						width: parent.width
						spacing: 6

						DialogField {
							id: renameInput
							width: parent.width - renameBtn.width - 6
							placeholderText: qsTr("Command name")
							onEdited: errorText = ""
							onAccepted: cmdCard.commitRename()
						}

						DialogButton {
							id: renameBtn
							text: qsTr("Rename")
							enabled: renameInput.text.trim() !== "" && renameInput.text.trim() !== cmdCard.cmdName
							onClicked: cmdCard.commitRename()
						}
					}
				}

				FormRow {
					label: qsTr("Description")
					badge: "description"
					showDivider: true
					showReset: { void section.editor.revision; return section.editor.contains(["command", cmdCard.cmdName, "description"]) }
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["command", cmdCard.cmdName, "description"])
					}

					DialogField {
						id: descField
						width: parent.width
						placeholderText: qsTr("Short summary shown in completion list")
						onEdited: cmdCard.cardMutate(() => section.editor.setStringAt(["command", cmdCard.cmdName, "description"], text))
					}
				}

				FormRow {
					label: qsTr("Prompt template")
					badge: "template"
					description: qsTr("Expanded and sent as the user prompt when the command runs.")
					stacked: true

					MultilineField {
						id: templateField
						width: parent.width
						monospace: true
						minHeight: 110
						placeholderText: qsTr("Review this diff:\n\n!`git diff`\n\n$ARGUMENTS")
						onEdited: cmdCard.cardMutate(() => section.editor.setStringAt(["command", cmdCard.cmdName, "template"], text, false))
					}
				}
			}
		}
	}

	Rectangle { width: parent.width; height: 1; color: Theme.border }

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 6
		text: qsTr("Instructions & Skills")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	FormRow {
		label: qsTr("Extra instruction files")
		badge: "instructions"
		description: qsTr("Paths or globs (relative, ~/…, or absolute) appended to the system prompt.")
		stacked: true
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["instructions"]) }
		onResetClicked: {
			section.forceActiveFocus()
			section.editor.removeAt(["instructions"])
		}

		StringListEditor {
			id: instructionsEditor
			width: parent.width
			monospace: true
			placeholderText: qsTr("docs/CONTRIBUTING.md or ~/notes/*.md")
			emptyText: qsTr("No extra instruction files")
			onValuesModified: vals => section.mutate(() => section.editor.setStringListAt(["instructions"], vals))
		}
	}

	FormRow {
		label: qsTr("Skill directories")
		badge: "skills"
		description: qsTr("Directories containing SKILL.md files offered to the model.")
		stacked: true
		showReset: { void section.editor.revision; return section.editor.contains(["skills"]) }
		onResetClicked: {
			section.forceActiveFocus()
			section.editor.removeAt(["skills"])
		}

		StringListEditor {
			id: skillsEditor
			width: parent.width
			monospace: true
			placeholderText: qsTr("./skills or ~/my-skills")
			emptyText: qsTr("Default skill directories only")
			onValuesModified: vals => section.writeSkillsList(vals)
		}
	}
}
