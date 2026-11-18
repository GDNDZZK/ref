pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 4: Global Permissions (`permission`), Tool Delays (`delay`),
// and legacy opencode `tools` map compatibility.
Column {
	id: section
	objectName: "configPermissionsSection"

	required property ConfigEditorController editor
	property bool mutating: false
	// "none" | "number" | "map"
	property string delayMode: "none"

	readonly property var delayModeTabs: [
		{ text: qsTr("No delay"), value: "none" },
		{ text: qsTr("All tools"), value: "number" },
		{ text: qsTr("Per-tool rules"), value: "map" }
	]

	function mutate(fn) {
		mutating = true
		fn()
		mutating = false
	}

	function syncControls() {
		permEditor.loadFromValue(editor.valueAt(["permission"]))

		const rawDelay = editor.valueAt(["delay"])
		if (rawDelay === undefined || rawDelay === null) {
			delayMode = "none"
			globalDelayField.value = undefined
			delayMapEditor.loadFromMap({})
		} else if (typeof rawDelay === "number" && Number.isFinite(rawDelay)) {
			delayMode = "number"
			globalDelayField.value = rawDelay
			delayMapEditor.loadFromMap({})
		} else if (typeof rawDelay === "object" && !Array.isArray(rawDelay)) {
			delayMode = "map"
			globalDelayField.value = undefined
			delayMapEditor.loadFromMap(rawDelay)
		} else {
			delayMode = "none"
			globalDelayField.value = undefined
			delayMapEditor.loadFromMap({})
		}

		const rawTools = editor.valueAt(["tools"])
		legacyToolsEditor.loadFromMap(typeof rawTools === "object" && rawTools !== null && !Array.isArray(rawTools) ? rawTools : {})
	}

	function applyPermission(newVal) {
		mutate(() => {
			if (newVal === undefined || newVal === null) {
				editor.removeAt(["permission"])
				return
			}
			if (typeof newVal === "object" && !Array.isArray(newVal) && Object.keys(newVal).length === 0) {
				editor.removeAt(["permission"])
				return
			}
			editor.setValueAt(["permission"], newVal)
		})
	}

	function switchDelayMode(targetMode) {
		if (delayMode === targetMode)
			return
		section.forceActiveFocus()
		delayMode = targetMode
		mutate(() => {
			if (targetMode === "none") {
				globalDelayField.value = undefined
				delayMapEditor.loadFromMap({})
				editor.removeAt(["delay"])
			} else if (targetMode === "number") {
				globalDelayField.value = 2
				editor.setValueAt(["delay"], 2)
			} else {
				const initial = { "bash": 2 }
				delayMapEditor.loadFromMap(initial)
				editor.setValueAt(["delay"], initial)
			}
		})
	}

	width: parent ? parent.width : 560
	spacing: 8

	Component.onCompleted: syncControls()

	Connections {
		target: section.editor
		function onParsedChanged() {
			if (!section.mutating)
				section.syncControls()
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Tool Permissions")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Rules are evaluated in order and the last matching rule wins. Use tool globs (bash, edit, webfetch, mcp_*) and argument patterns (e.g. git status*, *.lock).")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	FormRow {
		label: qsTr("Global permission policy")
		badge: "permission"
		stacked: true
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["permission"]) }
		onResetClicked: {
			section.forceActiveFocus()
			section.editor.removeAt(["permission"])
		}

		PermissionRulesEditor {
			id: permEditor
			objectName: "globalPermissionEditor"
			width: parent.width
			allowInherit: false
			onPermissionModified: newPerm => section.applyPermission(newPerm)
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 10
		text: qsTr("Tool Delays")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Wait a countdown in seconds before running matching tool calls so they can be reviewed or cancelled.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	FormRow {
		label: qsTr("Delay mode")
		badge: "delay"
		stacked: true
		showDivider: { void section.editor.revision; return section.editor.contains(["tools"]) }
		showReset: { void section.editor.revision; return section.editor.contains(["delay"]) }
		onResetClicked: {
			section.forceActiveFocus()
			section.editor.removeAt(["delay"])
		}

		Column {
			width: parent.width
			spacing: 10

			Segmented {
				width: parent.width
				model: section.delayModeTabs
				value: section.delayMode
				onActivated: m => section.switchDelayMode(m)
			}

			FormRow {
				visible: section.delayMode === "number"
				label: qsTr("Countdown for every tool")
				description: qsTr("Seconds to wait before executing any tool call.")
				controlWidth: 200

				NumberField {
					id: globalDelayField
					width: parent.width
					decimals: true
					minimum: 0
					step: 1
					suffix: "s"
					placeholderText: "2"
					onValueModified: val => section.mutate(() => {
						if (val === undefined)
							section.editor.removeAt(["delay"])
						else
							section.editor.setValueAt(["delay"], val)
					})
				}
			}

			KeyValueEditor {
				id: delayMapEditor
				visible: section.delayMode === "map"
				width: parent.width
				valueMode: "number"
				keyPlaceholder: qsTr("Tool glob (e.g. bash, delete, mcp_*)")
				valuePlaceholder: qsTr("Seconds (e.g. 2)")
				emptyText: qsTr("No per-tool delays")
				onMapModified: newMap => section.mutate(() => {
					if (Object.keys(newMap).length === 0)
						section.editor.removeAt(["delay"])
					else
						section.editor.setValueAt(["delay"], newMap)
				})
			}
		}
	}

	FormRow {
		visible: { void section.editor.revision; return section.editor.contains(["tools"]) }
		label: qsTr("Legacy tools map")
		badge: "tools"
		description: qsTr("Older opencode boolean map (e.g. {\"bash\": false}). Prefer permission rules above.")
		stacked: true
		showReset: true
		resetText: qsTr("Remove legacy map")
		onResetClicked: {
			section.forceActiveFocus()
			section.editor.removeAt(["tools"])
		}

		KeyValueEditor {
			id: legacyToolsEditor
			width: parent.width
			valueMode: "json"
			keyPlaceholder: qsTr("Tool name")
			valuePlaceholder: "false"
			onMapModified: newMap => section.mutate(() => {
				if (Object.keys(newMap).length === 0)
					section.editor.removeAt(["tools"])
				else
					section.editor.setValueAt(["tools"], newMap)
			})
		}
	}
}
