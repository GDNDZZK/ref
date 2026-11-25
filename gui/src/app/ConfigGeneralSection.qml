pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 1: Top-level General settings ($name, model, small_model, default_agent,
// compaction, retry, images, tui defaults, enabled_providers, disabled_providers).
Column {
	id: section
	objectName: "configGeneralSection"

	required property ConfigEditorController editor
	property bool mutating: false
	property var modelOpts: []
	property var agentOpts: []

	readonly property bool hasValidationError: compactionRecommendField.hasValidationError
		|| compactionThresholdField.hasValidationError
		|| retryMaxDelayField.hasValidationError
	readonly property bool hasPendingChanges: compactionRecommendField.hasUncommittedEdit
		|| compactionThresholdField.hasUncommittedEdit
		|| retryMaxDelayField.hasUncommittedEdit
		|| enabledProvidersEditor.hasUncommittedEdit
		|| disabledProvidersEditor.hasUncommittedEdit

	function mutate(fn) {
		mutating = true
		fn()
		mutating = false
	}

	function strAt(path) {
		const v = editor.valueAt(path)
		return typeof v === "string" ? v : ""
	}

	function numAt(path) {
		const v = editor.valueAt(path)
		return typeof v === "number" && Number.isFinite(v) ? v : undefined
	}

	function boolAt(path, fallback) {
		const v = editor.valueAt(path)
		return typeof v === "boolean" ? v : fallback
	}

	function listAt(path) {
		const v = editor.valueAt(path)
		return Fields.isArray(v) ? Fields.toArray(v).map(item => String(item ?? "")) : []
	}

	function resetPath(path) {
		section.forceActiveFocus()
		editor.removeAt(path)
	}

	function syncControls() {
		modelOpts = editor.modelOptions()
		agentOpts = editor.agentOptions()
		nameField.text = strAt(["$name"])
		modelSelect.value = strAt(["model"])
		smallModelSelect.value = strAt(["small_model"])
		defaultAgentSelect.value = strAt(["default_agent"])
		compactionRecommendField.value = numAt(["compaction", "recommend"])
		compactionThresholdField.value = numAt(["compaction", "threshold"])
		retryMaxDelayField.value = numAt(["retry", "max_delay"])
		enabledProvidersEditor.values = listAt(["enabled_providers"])
		disabledProvidersEditor.values = listAt(["disabled_providers"])
	}

	function commitPending() {
		const trimmedName = nameField.text.trim()
		if (nameField.text !== trimmedName) {
			nameField.text = trimmedName
			mutate(() => editor.setStringAt(["$name"], trimmedName))
		}
		const okEnabled = enabledProvidersEditor.commitPending()
		const okDisabled = disabledProvidersEditor.commitPending()
		return okEnabled && okDisabled && !hasValidationError
	}

	function resetSubEditors() {
		compactionRecommendField.resetState()
		compactionThresholdField.resetState()
		retryMaxDelayField.resetState()
		enabledProvidersEditor.resetState()
		disabledProvidersEditor.resetState()
		syncControls()
	}

	width: parent ? parent.width : 560
	spacing: 4

	Component.onCompleted: syncControls()

	Connections {
		target: section.editor
		function onParsedChanged() {
			if (!section.mutating)
				section.syncControls()
			else {
				section.modelOpts = section.editor.modelOptions()
				section.agentOpts = section.editor.agentOptions()
			}
		}
	}

	Connections {
		target: Workspace.config
		function onChanged() {
			section.modelOpts = section.editor.modelOptions()
			section.agentOpts = section.editor.agentOptions()
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		bottomPadding: 4
		text: qsTr("Profile & Defaults")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	FormRow {
		label: qsTr("Profile label")
		badge: "$name"
		description: qsTr("Friendly name shown in the profile switcher; ignored by the runtime.")
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["$name"]) }
		onResetClicked: section.resetPath(["$name"])

		DialogField {
			id: nameField
			objectName: "generalProfileNameField"
			width: parent.width
			placeholderText: qsTr("e.g. Work Anthropic, Local Ollama")
			onEdited: section.mutate(() => section.editor.setStringAt(["$name"], text.trim() === "" ? "" : text))
			onEditingFinished: {
				const trimmed = text.trim()
				if (text !== trimmed)
					text = trimmed
				section.mutate(() => section.editor.setStringAt(["$name"], trimmed))
			}
		}
	}

	FormRow {
		label: qsTr("Default model")
		badge: "model"
		description: qsTr("Model for agents that do not specify one (provider/model).")
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["model"]) }
		onResetClicked: section.resetPath(["model"])

		SelectField {
			id: modelSelect
			objectName: "generalModelSelect"
			width: parent.width
			editable: true
			clearable: true
			options: section.modelOpts
			placeholderText: qsTr("provider/model")
			onActivated: val => section.mutate(() => section.editor.setStringAt(["model"], val))
		}
	}

	FormRow {
		label: qsTr("Small model")
		badge: "small_model"
		description: qsTr("Lightweight model used to generate session titles.")
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["small_model"]) }
		onResetClicked: section.resetPath(["small_model"])

		SelectField {
			id: smallModelSelect
			objectName: "generalSmallModelSelect"
			width: parent.width
			editable: true
			clearable: true
			options: section.modelOpts
			placeholderText: qsTr("Same as session model")
			onActivated: val => section.mutate(() => section.editor.setStringAt(["small_model"], val))
		}
	}

	FormRow {
		label: qsTr("Default agent")
		badge: "default_agent"
		description: qsTr("Agent selected when starting a new session.")
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["default_agent"]) }
		onResetClicked: section.resetPath(["default_agent"])

		SelectField {
			id: defaultAgentSelect
			objectName: "generalDefaultAgentSelect"
			width: parent.width
			editable: true
			clearable: true
			options: section.agentOpts
			placeholderText: "build"
			onActivated: val => section.mutate(() => section.editor.setStringAt(["default_agent"], val))
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 14
		bottomPadding: 4
		text: qsTr("Context Compaction")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	FormRow {
		label: qsTr("Automatic compaction")
		badge: "compaction.auto"
		description: qsTr("Automatically summarize older turns when context reaches the threshold.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["compaction", "auto"]) }
		onResetClicked: section.resetPath(["compaction", "auto"])

		Switch {
			objectName: "compactionAutoSwitch"
			accessibleName: qsTr("Automatic compaction")
			checked: { void section.editor.revision; return section.boolAt(["compaction", "auto"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["compaction", "auto"], val))
		}
	}

	FormRow {
		label: qsTr("Recommended point")
		badge: "compaction.recommend"
		description: qsTr("Fraction (≤ 1, default 0.6) or token count (> 1) advised to the model.")
		controlWidth: 200
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["compaction", "recommend"]) }
		onResetClicked: { section.resetPath(["compaction", "recommend"]); compactionRecommendField.resetState() }

		NumberField {
			id: compactionRecommendField
			objectName: "compactionRecommendField"
			width: parent.width
			decimals: true
			minimum: 0
			exclusiveMinimum: true
			step: 0.05
			placeholderText: "0.6"
			onValueModified: val => section.mutate(() => section.editor.setValueAt(["compaction", "recommend"], val))
		}
	}

	FormRow {
		label: qsTr("Automatic threshold")
		badge: "compaction.threshold"
		description: qsTr("Fraction (≤ 1, default 0.9) or token count (> 1) that triggers compaction.")
		controlWidth: 200
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["compaction", "threshold"]) }
		onResetClicked: { section.resetPath(["compaction", "threshold"]); compactionThresholdField.resetState() }

		NumberField {
			id: compactionThresholdField
			objectName: "compactionThresholdField"
			width: parent.width
			decimals: true
			minimum: 0
			exclusiveMinimum: true
			step: 0.05
			placeholderText: "0.9"
			onValueModified: val => section.mutate(() => section.editor.setValueAt(["compaction", "threshold"], val))
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 14
		bottomPadding: 4
		text: qsTr("Retries, Images & TUI Defaults")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	FormRow {
		label: qsTr("Maximum retry backoff")
		badge: "retry.max_delay"
		description: qsTr("Cap in seconds for exponential backoff when retrying failed LLM requests.")
		controlWidth: 200
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["retry", "max_delay"]) }
		onResetClicked: { section.resetPath(["retry", "max_delay"]); retryMaxDelayField.resetState() }

		NumberField {
			id: retryMaxDelayField
			objectName: "retryMaxDelayField"
			width: parent.width
			minimum: 0
			step: 10
			suffix: "s"
			placeholderText: "600"
			onValueModified: val => section.mutate(() => section.editor.setValueAt(["retry", "max_delay"], val))
		}
	}

	FormRow {
		label: qsTr("Respect retry response headers")
		badge: "retry.respect_headers"
		description: qsTr("Honor Retry-After and rate-limit reset headers from upstream responses (highest precedence).")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["retry", "respect_headers"]) }
		onResetClicked: section.resetPath(["retry", "respect_headers"])

		Switch {
			objectName: "retryRespectHeadersSwitch"
			accessibleName: qsTr("Respect retry response headers")
			checked: { void section.editor.revision; return section.boolAt(["retry", "respect_headers"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["retry", "respect_headers"], val))
		}
	}

	FormRow {
		label: qsTr("Respect retry body fields")
		badge: "retry.respect_body_fields"
		description: qsTr("Honor structured retry_after and retryDelay fields in JSON or SSE error payloads.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["retry", "respect_body_fields"]) }
		onResetClicked: section.resetPath(["retry", "respect_body_fields"])

		Switch {
			objectName: "retryRespectBodyFieldsSwitch"
			accessibleName: qsTr("Respect retry body fields")
			checked: { void section.editor.revision; return section.boolAt(["retry", "respect_body_fields"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["retry", "respect_body_fields"], val))
		}
	}

	FormRow {
		label: qsTr("Parse retry hints from error text")
		badge: "retry.parse_error_text"
		description: qsTr("Extract retry wait durations from free-form error messages when no higher-precedence hint is present.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["retry", "parse_error_text"]) }
		onResetClicked: section.resetPath(["retry", "parse_error_text"])

		Switch {
			objectName: "retryParseErrorTextSwitch"
			accessibleName: qsTr("Parse retry hints from error text")
			checked: { void section.editor.revision; return section.boolAt(["retry", "parse_error_text"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["retry", "parse_error_text"], val))
		}
	}

	FormRow {
		label: qsTr("Auto-resize images")
		badge: "images.auto_resize"
		description: qsTr("Downscale and recompress read, fetched, and pasted images for models.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["images", "auto_resize"]) }
		onResetClicked: section.resetPath(["images", "auto_resize"])

		Switch {
			accessibleName: qsTr("Auto-resize images")
			checked: { void section.editor.revision; return section.boolAt(["images", "auto_resize"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["images", "auto_resize"], val))
		}
	}

	FormRow {
		label: qsTr("Show sidebar by default")
		badge: "tui.sidebar"
		description: qsTr("Initial sidebar visibility in TUI sessions.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["tui", "sidebar"]) }
		onResetClicked: section.resetPath(["tui", "sidebar"])

		Switch {
			accessibleName: qsTr("Show sidebar by default")
			checked: { void section.editor.revision; return section.boolAt(["tui", "sidebar"], true) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["tui", "sidebar"], val))
		}
	}

	FormRow {
		label: qsTr("Expand thinking by default")
		badge: "tui.thinking"
		description: qsTr("Expand reasoning blocks initially in new sessions.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["tui", "thinking"]) }
		onResetClicked: section.resetPath(["tui", "thinking"])

		Switch {
			accessibleName: qsTr("Expand thinking by default")
			checked: { void section.editor.revision; return section.boolAt(["tui", "thinking"], false) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["tui", "thinking"], val))
		}
	}

	FormRow {
		label: qsTr("Expand tool details by default")
		badge: "tui.details"
		description: qsTr("Expand tool call inputs and outputs initially in new sessions.")
		controlWidth: 0
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["tui", "details"]) }
		onResetClicked: section.resetPath(["tui", "details"])

		Switch {
			accessibleName: qsTr("Expand tool details by default")
			checked: { void section.editor.revision; return section.boolAt(["tui", "details"], false) }
			onToggled: val => section.mutate(() => section.editor.setValueAt(["tui", "details"], val))
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		topPadding: 14
		bottomPadding: 4
		text: qsTr("Provider Filters")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	FormRow {
		label: qsTr("Enabled providers")
		badge: "enabled_providers"
		description: qsTr("When non-empty, only these provider IDs are loaded.")
		stacked: true
		showDivider: true
		showReset: { void section.editor.revision; return section.editor.contains(["enabled_providers"]) }
		onResetClicked: { section.resetPath(["enabled_providers"]); enabledProvidersEditor.resetState() }

		StringListEditor {
			id: enabledProvidersEditor
			objectName: "enabledProvidersEditor"
			width: parent.width
			monospace: true
			placeholderText: qsTr("Provider ID (e.g. anthropic)")
			emptyText: qsTr("All configured providers enabled")
			onValuesModified: vals => section.mutate(() => section.editor.setStringListAt(["enabled_providers"], vals))
		}
	}

	FormRow {
		label: qsTr("Disabled providers")
		badge: "disabled_providers"
		description: qsTr("Provider IDs listed here are skipped even if configured or in environment.")
		stacked: true
		showReset: { void section.editor.revision; return section.editor.contains(["disabled_providers"]) }
		onResetClicked: { section.resetPath(["disabled_providers"]); disabledProvidersEditor.resetState() }

		StringListEditor {
			id: disabledProvidersEditor
			objectName: "disabledProvidersEditor"
			width: parent.width
			monospace: true
			placeholderText: qsTr("Provider ID to disable")
			emptyText: qsTr("No providers disabled")
			onValuesModified: vals => section.mutate(() => section.editor.setStringListAt(["disabled_providers"], vals))
		}
	}
}
