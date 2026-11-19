pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Visual editor section 2: Providers (`provider.<id>`) with quick presets, nested Models CRUD,
// and nested Variants CRUD.
Column {
	id: section
	objectName: "configProvidersSection"

	required property ConfigEditorController editor
	property bool mutating: false
	property var providerKeys: []
	property var expandedProviders: ({})
	property var expandedModels: ({})
	property bool showingCustomInput: false
	property int cardsRevision: 0

	readonly property var presets: [
		{ id: "anthropic", label: "Anthropic" },
		{ id: "openai", label: "OpenAI" },
		{ id: "deepseek", label: "DeepSeek" },
		{ id: "openrouter", label: "OpenRouter" },
		{ id: "ollama", label: qsTr("Ollama / Local") }
	]
	readonly property var knownProviderOptionKeys: [
		"baseURL", "baseUrl", "base_url",
		"apiKey", "api_key",
		"headers", "setCacheKey", "chunkTimeout"
	]

	readonly property bool hasValidationError: {
		void cardsRevision
		void providerKeys
		if (showingCustomInput && customProviderIdField.hasError)
			return true
		for (let i = 0; i < provRepeater.count; ++i) {
			const c = provRepeater.itemAt(i) as ProvCard
			if (c && c.hasValidationError)
				return true
		}
		return false
	}

	readonly property bool hasPendingChanges: {
		void cardsRevision
		void providerKeys
		if (showingCustomInput && customProviderIdField.text.trim() !== "")
			return true
		for (let i = 0; i < provRepeater.count; ++i) {
			const c = provRepeater.itemAt(i) as ProvCard
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
		const next = editor.keysAt(["provider"])
		if (!sameKeys(providerKeys, next))
			providerKeys = next
	}

	function isProviderExpanded(id) {
		return Boolean(expandedProviders[id])
	}

	function setProviderExpanded(id, open) {
		const next = Object.assign({}, expandedProviders)
		next[id] = open
		expandedProviders = next
	}

	function renameProviderExpanded(oldId, newId) {
		const nextProv = Object.assign({}, expandedProviders)
		const wasOpen = Boolean(nextProv[oldId])
		delete nextProv[oldId]
		nextProv[newId] = wasOpen
		expandedProviders = nextProv

		const oldPrefix = oldId + "/"
		const nextModels = {}
		for (const k of Object.keys(expandedModels)) {
			if (k.startsWith(oldPrefix))
				nextModels[newId + "/" + k.slice(oldPrefix.length)] = expandedModels[k]
			else
				nextModels[k] = expandedModels[k]
		}
		expandedModels = nextModels
	}

	function isModelExpanded(providerId, modelKey) {
		return Boolean(expandedModels[providerId + "/" + modelKey])
	}

	function setModelExpanded(providerId, modelKey, open) {
		const next = Object.assign({}, expandedModels)
		next[providerId + "/" + modelKey] = open
		expandedModels = next
	}

	function renameModelExpanded(providerId, oldModelKey, newModelKey) {
		const oldComposite = providerId + "/" + oldModelKey
		const newComposite = providerId + "/" + newModelKey
		const next = Object.assign({}, expandedModels)
		const wasOpen = Boolean(next[oldComposite])
		delete next[oldComposite]
		next[newComposite] = wasOpen
		expandedModels = next
	}

	function triggerPreset(presetId) {
		const created = editor.addProviderPreset(presetId)
		if (created !== "")
			setProviderExpanded(created, true)
	}

	function addCustomProvider() {
		const requested = customProviderIdField.text.trim()
		if (requested === "") {
			customProviderIdField.errorText = qsTr("Enter a provider ID.")
			return false
		}
		const created = editor.addProviderPreset("custom", requested)
		if (created === "") {
			customProviderIdField.errorText = qsTr("Provider \"%1\" already exists.").arg(requested)
			return false
		}
		customProviderIdField.errorText = ""
		customProviderIdField.text = ""
		showingCustomInput = false
		setProviderExpanded(created, true)
		return true
	}

	function commitPending() {
		if (showingCustomInput && customProviderIdField.text.trim() !== "" && !addCustomProvider())
			return false
		for (let i = 0; i < provRepeater.count; ++i) {
			const c = provRepeater.itemAt(i) as ProvCard
			if (c && !c.commitPending())
				return false
		}
		return !hasValidationError
	}

	function resetSubEditors() {
		showingCustomInput = false
		customProviderIdField.text = ""
		customProviderIdField.errorText = ""
		for (let i = 0; i < provRepeater.count; ++i) {
			const c = provRepeater.itemAt(i) as ProvCard
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

	component VariantCard: Rectangle {
		id: varBox
		required property string providerId
		required property string modelKey
		required property string modelData
		readonly property string variantKey: modelData
		property bool varMutating: false

		signal variantEntriesEdited(string vKey, var nextEntries)

		readonly property var variantPath: ["provider", providerId, "models", modelKey, "variants", variantKey]
		readonly property bool hasValidationError: varOptsEditor.hasValidationError
		readonly property bool hasPendingChanges: varOptsEditor.hasUncommittedEdit

		function syncVariant() {
			varOptsEditor.loadFromEntries(section.editor.orderedEntriesAt(variantPath))
		}

		function commitPending() {
			return varOptsEditor.commitPending()
		}

		function resetVariantState() {
			varOptsEditor.resetState()
			syncVariant()
		}

		width: parent ? parent.width : 400
		implicitHeight: varInner.implicitHeight + 20
		radius: Theme.radiusSmall
		color: Theme.background
		border.color: Theme.border

		Component.onCompleted: syncVariant()

		Connections {
			target: section.editor
			function onParsedChanged() {
				if (!varBox.varMutating)
					varBox.syncVariant()
			}
		}

		Column {
			id: varInner
			x: 10
			y: 10
			width: parent.width - 20
			spacing: 6

			Item {
				width: parent.width
				height: 28

				Text {
					textFormat: Text.PlainText
					anchors { left: parent.left; verticalCenter: parent.verticalCenter }
					text: "#" + varBox.variantKey
					color: Theme.accentText
					font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize; bold: true }
				}

				IconButton {
					anchors { right: parent.right; verticalCenter: parent.verticalCenter }
					buttonSize: 24
					iconName: "trash"
					iconSize: 16
					tooltipText: qsTr("Delete variant")
					Accessible.name: qsTr("Delete variant")
					onClicked: {
						section.editor.removeAt(varBox.variantPath, false)
						if (section.editor.keysAt(["provider", varBox.providerId, "models", varBox.modelKey, "variants"]).length === 0)
							section.editor.removeAt(["provider", varBox.providerId, "models", varBox.modelKey, "variants"], false)
					}
				}
			}

			KeyValueEditor {
				id: varOptsEditor
				objectName: "variantOptionsEditor_" + varBox.providerId + "_" + varBox.modelKey + "_" + varBox.variantKey
				width: parent.width
				valueMode: "json"
				keyPlaceholder: "reasoningEffort"
				valuePlaceholder: "\"high\""
				emptyText: qsTr("No variant fields")
				onEntriesModified: nextEntries => {
					varBox.varMutating = true
					varBox.variantEntriesEdited(varBox.variantKey, nextEntries)
					varBox.varMutating = false
				}
			}
		}
	}

	component ModelCard: Rectangle {
		id: modelCard
		required property string providerId
		required property string modelData
		readonly property string modelKey: modelData
		readonly property bool mExpanded: section.isModelExpanded(providerId, modelKey)
		property bool modelMutating: false
		property var variantKeys: []
		property int variantsRevision: 0

		signal beginParentMutate()
		signal endParentMutate()

		readonly property string pendingModelRenameTarget: modelRenameField.text.trim()
		readonly property bool variantsHaveValidationError: {
			void variantsRevision
			void variantKeys
			for (let i = 0; i < varRepeater.count; ++i) {
				const v = varRepeater.itemAt(i) as VariantCard
				if (v && v.hasValidationError)
					return true
			}
			return false
		}
		readonly property bool variantsHavePendingChanges: {
			void variantsRevision
			void variantKeys
			for (let i = 0; i < varRepeater.count; ++i) {
				const v = varRepeater.itemAt(i) as VariantCard
				if (v && v.hasPendingChanges)
					return true
			}
			return false
		}
		readonly property bool hasValidationError: modelRenameField.hasError
			|| newVariantField.hasError
			|| ctxLimitField.hasValidationError
			|| outLimitField.hasValidationError
			|| compactLimitField.hasValidationError
			|| threshLimitField.hasValidationError
			|| mTempField.hasValidationError
			|| mTopPField.hasValidationError
			|| mHeadersEditor.hasValidationError
			|| mOptionsEditor.hasValidationError
			|| variantsHaveValidationError
		readonly property bool hasPendingChanges: (pendingModelRenameTarget !== "" && pendingModelRenameTarget !== modelKey)
			|| newVariantField.text.trim() !== ""
			|| ctxLimitField.hasUncommittedEdit
			|| outLimitField.hasUncommittedEdit
			|| compactLimitField.hasUncommittedEdit
			|| threshLimitField.hasUncommittedEdit
			|| mTempField.hasUncommittedEdit
			|| mTopPField.hasUncommittedEdit
			|| mHeadersEditor.hasUncommittedEdit
			|| mOptionsEditor.hasUncommittedEdit
			|| variantsHavePendingChanges

		function mPath(tail) {
			return ["provider", providerId, "models", modelKey].concat(tail)
		}

		function mMutate(fn) {
			modelMutating = true
			beginParentMutate()
			fn()
			endParentMutate()
			modelMutating = false
		}

		function mStr(tail) {
			const v = section.editor.valueAt(mPath(tail))
			return typeof v === "string" ? v : ""
		}

		function mNum(tail) {
			const v = section.editor.valueAt(mPath(tail))
			return typeof v === "number" && Number.isFinite(v) ? v : undefined
		}

		function mBool(tail, fallback) {
			const v = section.editor.valueAt(mPath(tail))
			return typeof v === "boolean" ? v : fallback
		}

		function hasImageModality() {
			return Fields.toArray(section.editor.valueAt(mPath(["modalities", "input"]))).includes("image")
		}

		function readAttachment() {
			if (section.editor.contains(mPath(["attachment"])))
				return mBool(["attachment"], false) || hasImageModality()
			return hasImageModality()
		}

		function stripImageModality() {
			const inp = Fields.toArray(section.editor.valueAt(mPath(["modalities", "input"])))
			if (!inp.includes("image"))
				return
			const remaining = inp.filter(x => x !== "image").map(x => String(x ?? ""))
			if (remaining.length > 0) {
				section.editor.setStringListAt(mPath(["modalities", "input"]), remaining, false)
				return
			}
			section.editor.removeAt(mPath(["modalities", "input"]), false)
			if (section.editor.keysAt(mPath(["modalities"])).length === 0)
				section.editor.removeAt(mPath(["modalities"]), false)
		}

		function readReasoning() {
			const primary = section.editor.valueAt(mPath(["reasoning"]))
			if (typeof primary === "boolean")
				return primary
			const alias = section.editor.valueAt(mPath(["thinking"]))
			if (typeof alias === "boolean")
				return alias
			return false
		}

		function pruneEmptyModelLimit() {
			if (section.editor.keysAt(mPath(["limit"])).length === 0)
				section.editor.removeAt(mPath(["limit"]), false)
		}

		function setModelLimit(limitKey, val) {
			if (val === undefined) {
				section.editor.removeAt(mPath(["limit", limitKey]), false)
				pruneEmptyModelLimit()
			} else {
				section.editor.setValueAt(mPath(["limit", limitKey]), val)
			}
		}

		function resetLimit(limitKey, field) {
			section.forceActiveFocus()
			mMutate(() => setModelLimit(limitKey, undefined))
			field.value = undefined
			field.resetState()
		}

		function syncModelCard() {
			const nextVars = section.editor.keysAt(mPath(["variants"]))
			if (!section.sameKeys(variantKeys, nextVars))
				variantKeys = nextVars

			modelRenameField.text = modelKey
			modelRenameField.errorText = ""
			apiIdField.text = mStr(["id"])
			modelNameField.text = mStr(["name"])
			ctxLimitField.value = mNum(["limit", "context"])
			outLimitField.value = mNum(["limit", "output"])
			compactLimitField.value = mNum(["limit", "compact"])
			threshLimitField.value = mNum(["limit", "threshold"])
			mTempField.value = mNum(["temperature"])
			mTopPField.value = mNum(["top_p"])
			mHeadersEditor.loadFromEntries(section.editor.orderedEntriesAt(mPath(["headers"])))
			mOptionsEditor.loadFromEntries(section.editor.orderedEntriesAt(mPath(["options"])))
		}

		function validateModelRenameLive() {
			const target = modelRenameField.text.trim()
			if (target !== "" && target !== modelKey && section.editor.keysAt(["provider", providerId, "models"]).includes(target))
				modelRenameField.errorText = qsTr("Model \"%1\" already exists.").arg(target)
			else
				modelRenameField.errorText = ""
		}

		function commitModelRename() {
			const target = modelRenameField.text.trim()
			if (target === "" || target === modelKey) {
				modelRenameField.text = modelKey
				modelRenameField.errorText = ""
				return true
			}
			if (section.editor.keysAt(["provider", providerId, "models"]).includes(target)) {
				modelRenameField.errorText = qsTr("Model \"%1\" already exists.").arg(target)
				return false
			}
			modelRenameField.errorText = ""
			section.renameModelExpanded(providerId, modelKey, target)
			section.editor.renameKeyAt(["provider", providerId, "models"], modelKey, target)
			return true
		}

		function addVariant() {
			const vKey = newVariantField.text.trim()
			if (vKey === "") {
				newVariantField.errorText = qsTr("Enter a variant name.")
				return false
			}
			if (section.editor.keysAt(mPath(["variants"])).includes(vKey)) {
				newVariantField.errorText = qsTr("Variant \"%1\" already exists.").arg(vKey)
				return false
			}
			newVariantField.errorText = ""
			newVariantField.text = ""
			section.editor.setValueAt(mPath(["variants", vKey]), {})
			return true
		}

		function commitPending() {
			const trimmedModelName = modelNameField.text.trim()
			if (modelNameField.text !== trimmedModelName) {
				modelNameField.text = trimmedModelName
				mMutate(() => {
					if (trimmedModelName === "")
						section.editor.removeAt(mPath(["name"]), false)
					else
						section.editor.setValueAt(mPath(["name"]), trimmedModelName)
				})
			}
			if (newVariantField.text.trim() !== "" && !addVariant())
				return false
			if (!mHeadersEditor.commitPending() || !mOptionsEditor.commitPending())
				return false
			for (let i = 0; i < varRepeater.count; ++i) {
				const v = varRepeater.itemAt(i) as VariantCard
				if (v && !v.commitPending())
					return false
			}
			if (hasValidationError)
				return false
			return commitModelRename()
		}

		function resetModelState() {
			newVariantField.text = ""
			newVariantField.errorText = ""
			ctxLimitField.resetState()
			outLimitField.resetState()
			compactLimitField.resetState()
			threshLimitField.resetState()
			mTempField.resetState()
			mTopPField.resetState()
			mHeadersEditor.resetState()
			mOptionsEditor.resetState()
			for (let i = 0; i < varRepeater.count; ++i) {
				const v = varRepeater.itemAt(i) as VariantCard
				if (v)
					v.resetVariantState()
			}
			syncModelCard()
		}

		width: parent ? parent.width : 480
		implicitHeight: mHeader.height + (mExpanded ? mBody.implicitHeight + 14 : 0)
		radius: Theme.radiusSmall + 2
		color: Theme.surface
		border.color: Theme.border
		clip: true

		Component.onCompleted: syncModelCard()

		Connections {
			target: section.editor
			function onParsedChanged() {
				if (!modelCard.modelMutating)
					modelCard.syncModelCard()
				else {
					const nextVars = section.editor.keysAt(modelCard.mPath(["variants"]))
					if (!section.sameKeys(modelCard.variantKeys, nextVars))
						modelCard.variantKeys = nextVars
				}
			}
		}

		Item {
			id: mHeader
			width: parent.width
			height: 40

			Item {
				anchors { left: parent.left; right: delModelBtn.left; top: parent.top; bottom: parent.bottom }

				Chevron {
					id: mChev
					x: 12
					anchors.verticalCenter: parent.verticalCenter
					rotation: modelCard.mExpanded ? 90 : 0
					color: Theme.textMuted
					Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				Text {
					id: mKeyLabel
					textFormat: Text.PlainText
					anchors { left: mChev.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					text: modelCard.modelKey
					color: {
						void section.editor.revision
						return modelCard.mBool(["disabled"], false) ? Theme.textMuted : Theme.text
					}
					font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize; bold: true }
				}

				Text {
					textFormat: Text.PlainText
					anchors { left: mKeyLabel.right; leftMargin: 8; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					text: {
						void section.editor.revision
						if (modelCard.mBool(["disabled"], false))
							return qsTr("(disabled)")
						return modelCard.mStr(["name"])
					}
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: section.setModelExpanded(modelCard.providerId, modelCard.modelKey, !modelCard.mExpanded) }
			}

			IconButton {
				id: delModelBtn
				anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
				buttonSize: 26
				iconName: "trash"
				iconSize: 16
				tooltipText: qsTr("Delete model")
				Accessible.name: qsTr("Delete model")
				onClicked: section.editor.removeAt(["provider", modelCard.providerId, "models", modelCard.modelKey], false)
			}
		}

		Column {
			id: mBody
			visible: modelCard.mExpanded
			y: mHeader.height
			x: 12
			width: parent.width - 24
			spacing: 4

			Rectangle { width: parent.width; height: 1; color: Theme.border }

			FormRow {
				label: qsTr("Model key")
				showDivider: true

				Row {
					width: parent.width
					spacing: 6

					DialogField {
						id: modelRenameField
						objectName: "modelRenameField_" + modelCard.providerId + "_" + modelCard.modelKey
						width: parent.width - mRenameBtn.width - 6
						placeholderText: qsTr("Model key")
						onEdited: modelCard.validateModelRenameLive()
						onAccepted: modelCard.commitModelRename()
						onEditingFinished: modelCard.commitModelRename()
					}

					DialogButton {
						id: mRenameBtn
						text: qsTr("Rename")
						enabled: modelCard.pendingModelRenameTarget !== ""
							&& modelCard.pendingModelRenameTarget !== modelCard.modelKey
							&& !modelRenameField.hasError
						onClicked: modelCard.commitModelRename()
					}
				}
			}

			FormRow {
				label: qsTr("API model ID")
				badge: "id"
				description: qsTr("Sent in API requests (defaults to the model key).")
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["id"])) }
				onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["id"]), false) }

				DialogField {
					id: apiIdField
					width: parent.width
					placeholderText: modelCard.modelKey
					onEdited: modelCard.mMutate(() => {
						if (text.trim() === "")
							section.editor.removeAt(modelCard.mPath(["id"]), false)
						else
							section.editor.setValueAt(modelCard.mPath(["id"]), text.trim())
					})
					onEditingFinished: {
						const trimmed = text.trim()
						if (text !== trimmed)
							text = trimmed
					}
				}
			}

			FormRow {
				label: qsTr("Display name")
				badge: "name"
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["name"])) }
				onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["name"]), false) }

				DialogField {
					id: modelNameField
					width: parent.width
					placeholderText: modelCard.modelKey
					onEdited: modelCard.mMutate(() => {
						if (text.trim() === "")
							section.editor.removeAt(modelCard.mPath(["name"]), false)
						else
							section.editor.setValueAt(modelCard.mPath(["name"]), text)
					})
					onEditingFinished: {
						const trimmed = text.trim()
						if (text !== trimmed)
							text = trimmed
						modelCard.mMutate(() => {
							if (trimmed === "")
								section.editor.removeAt(modelCard.mPath(["name"]), false)
							else
								section.editor.setValueAt(modelCard.mPath(["name"]), trimmed)
						})
					}
				}
			}

			FormRow {
				label: qsTr("Accepts image attachments")
				badge: "attachment"
				controlWidth: 0
				showDivider: true
				showReset: {
					void section.editor.revision
					return section.editor.contains(modelCard.mPath(["attachment"])) || modelCard.hasImageModality()
				}
				onResetClicked: {
					section.forceActiveFocus()
					modelCard.mMutate(() => {
						modelCard.stripImageModality()
						section.editor.removeAt(modelCard.mPath(["attachment"]), false)
					})
				}

				Switch {
					objectName: "modelAttachmentSwitch_" + modelCard.providerId + "_" + modelCard.modelKey
					accessibleName: qsTr("Accepts image attachments")
					checked: { void section.editor.revision; return modelCard.readAttachment() }
					onToggled: val => modelCard.mMutate(() => {
						if (!val)
							modelCard.stripImageModality()
						section.editor.setValueAt(modelCard.mPath(["attachment"]), val)
					})
				}
			}

			FormRow {
				label: qsTr("Reasoning model")
				badge: "reasoning"
				controlWidth: 0
				showDivider: true
				showReset: {
					void section.editor.revision
					return section.editor.contains(modelCard.mPath(["reasoning"]))
						|| section.editor.contains(modelCard.mPath(["thinking"]))
				}
				onResetClicked: {
					section.forceActiveFocus()
					modelCard.mMutate(() => {
						section.editor.removeAt(modelCard.mPath(["thinking"]), false)
						section.editor.removeAt(modelCard.mPath(["reasoning"]), false)
					})
				}

				Switch {
					objectName: "modelReasoningSwitch_" + modelCard.providerId + "_" + modelCard.modelKey
					accessibleName: qsTr("Reasoning model")
					checked: { void section.editor.revision; return modelCard.readReasoning() }
					onToggled: val => modelCard.mMutate(() => {
						section.editor.removeAt(modelCard.mPath(["thinking"]), false)
						section.editor.setValueAt(modelCard.mPath(["reasoning"]), val)
					})
				}
			}

			FormRow {
				label: qsTr("Disable model")
				badge: "disabled"
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["disabled"])) }
				onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["disabled"]), false) }

				Switch {
					accessibleName: qsTr("Disable model")
					checked: { void section.editor.revision; return modelCard.mBool(["disabled"], false) }
					onToggled: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["disabled"]), val))
				}
			}

			FormRow {
				label: qsTr("Context window")
				badge: "limit.context"
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "context"])) }
				onResetClicked: modelCard.resetLimit("context", ctxLimitField)

				NumberField {
					id: ctxLimitField
					objectName: "modelContextField_" + modelCard.providerId + "_" + modelCard.modelKey
					width: parent.width
					minimum: 1
					step: 8192
					placeholderText: "128000"
					onValueModified: val => modelCard.mMutate(() => modelCard.setModelLimit("context", val))
				}
			}

			FormRow {
				label: qsTr("Max output tokens")
				badge: "limit.output"
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "output"])) }
				onResetClicked: modelCard.resetLimit("output", outLimitField)

				NumberField {
					id: outLimitField
					width: parent.width
					minimum: 1
					step: 4096
					placeholderText: "32000"
					onValueModified: val => modelCard.mMutate(() => modelCard.setModelLimit("output", val))
				}
			}

			FormRow {
				label: qsTr("Recommended compaction tokens")
				badge: "limit.compact"
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "compact"])) }
				onResetClicked: modelCard.resetLimit("compact", compactLimitField)

				NumberField {
					id: compactLimitField
					width: parent.width
					minimum: 1
					step: 8192
					placeholderText: qsTr("Global default")
					onValueModified: val => modelCard.mMutate(() => modelCard.setModelLimit("compact", val))
				}
			}

			FormRow {
				label: qsTr("Auto-compaction threshold")
				badge: "limit.threshold"
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "threshold"])) }
				onResetClicked: modelCard.resetLimit("threshold", threshLimitField)

				NumberField {
					id: threshLimitField
					objectName: "modelThresholdField_" + modelCard.providerId + "_" + modelCard.modelKey
					width: parent.width
					decimals: true
					minimum: 0
					exclusiveMinimum: true
					step: 0.05
					placeholderText: qsTr("Global default")
					onValueModified: val => modelCard.mMutate(() => modelCard.setModelLimit("threshold", val))
				}
			}

			FormRow {
				label: qsTr("Temperature")
				badge: "temperature"
				controlWidth: 180
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["temperature"])) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(modelCard.mPath(["temperature"]), false)
					mTempField.resetState()
				}

				NumberField {
					id: mTempField
					width: parent.width
					decimals: true
					minimum: 0
					maximum: 2
					step: 0.1
					placeholderText: qsTr("Provider default")
					onValueModified: val => modelCard.mMutate(() => {
						if (val === undefined)
							section.editor.removeAt(modelCard.mPath(["temperature"]), false)
						else
							section.editor.setValueAt(modelCard.mPath(["temperature"]), val)
					})
				}
			}

			FormRow {
				label: qsTr("Top-p")
				badge: "top_p"
				controlWidth: 180
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["top_p"])) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(modelCard.mPath(["top_p"]), false)
					mTopPField.resetState()
				}

				NumberField {
					id: mTopPField
					width: parent.width
					decimals: true
					minimum: 0
					maximum: 1
					step: 0.05
					placeholderText: qsTr("Provider default")
					onValueModified: val => modelCard.mMutate(() => {
						if (val === undefined)
							section.editor.removeAt(modelCard.mPath(["top_p"]), false)
						else
							section.editor.setValueAt(modelCard.mPath(["top_p"]), val)
					})
				}
			}

			FormRow {
				label: qsTr("Model HTTP headers")
				badge: "headers"
				stacked: true
				showDivider: true

				KeyValueEditor {
					id: mHeadersEditor
					objectName: "modelHeadersEditor_" + modelCard.providerId + "_" + modelCard.modelKey
					width: parent.width
					valueMode: "string"
					envStatus: section.editor.envStatus
					keyPlaceholder: "X-Model"
					valuePlaceholder: "1"
					emptyText: qsTr("No per-model headers")
					onEntriesModified: nextEntries => modelCard.mMutate(() => {
						section.editor.setOrderedObjectAt(modelCard.mPath(["headers"]), nextEntries, true, false)
					})
				}
			}

			FormRow {
				label: qsTr("Model request options")
				badge: "options"
				description: qsTr("JSON request options (e.g. reasoningEffort, thinking, extraBody).")
				stacked: true
				showDivider: true

				KeyValueEditor {
					id: mOptionsEditor
					objectName: "modelOptionsEditor_" + modelCard.providerId + "_" + modelCard.modelKey
					width: parent.width
					valueMode: "json"
					keyPlaceholder: "reasoningEffort"
					valuePlaceholder: "\"medium\""
					emptyText: qsTr("No model request options")
					onEntriesModified: nextEntries => modelCard.mMutate(() => {
						section.editor.setOrderedObjectAt(modelCard.mPath(["options"]), nextEntries, true, false)
					})
				}
			}

			// ---- Nested Variants CRUD ----
			FormRow {
				label: qsTr("Variants")
				badge: "variants"
				description: qsTr("Named option presets selected via provider/model#variant or /variant.")
				stacked: true

				Column {
					width: parent.width
					spacing: 8

					Repeater {
						id: varRepeater
						model: modelCard.variantKeys
						onItemAdded: modelCard.variantsRevision++
						onItemRemoved: modelCard.variantsRevision++

						delegate: VariantCard {
							providerId: modelCard.providerId
							modelKey: modelCard.modelKey
							onVariantEntriesEdited: (vKey, nextEntries) => {
								modelCard.mMutate(() => section.editor.setOrderedObjectAt(modelCard.mPath(["variants", vKey]), nextEntries, false, false))
							}
						}
					}

					Item {
						width: parent.width
						implicitHeight: newVariantField.implicitHeight

						DialogField {
							id: newVariantField
							anchors { left: parent.left; right: addVarBtn.left; rightMargin: 8; top: parent.top }
							placeholderText: qsTr("Variant name (e.g. high, low, off)")
							onEdited: errorText = ""
							onAccepted: modelCard.addVariant()
						}

						DialogButton {
							id: addVarBtn
							anchors { right: parent.right; top: parent.top }
							text: qsTr("+ Add Variant")
							onClicked: modelCard.addVariant()
						}
					}
				}
			}
		}
	}

	component ProvCard: Rectangle {
		id: provCard
		required property string modelData
		readonly property string providerId: modelData
		readonly property bool expanded: section.isProviderExpanded(providerId)
		property bool cardMutating: false
		property var modelKeys: []
		property int modelsRevision: 0

		readonly property string pendingRenameTarget: provRenameField.text.trim()
		readonly property bool modelsHaveValidationError: {
			void modelsRevision
			void modelKeys
			for (let i = 0; i < modelRepeater.count; ++i) {
				const m = modelRepeater.itemAt(i) as ModelCard
				if (m && m.hasValidationError)
					return true
			}
			return false
		}
		readonly property bool modelsHavePendingChanges: {
			void modelsRevision
			void modelKeys
			for (let i = 0; i < modelRepeater.count; ++i) {
				const m = modelRepeater.itemAt(i) as ModelCard
				if (m && m.hasPendingChanges)
					return true
			}
			return false
		}
		readonly property bool hasValidationError: provRenameField.hasError
			|| newModelField.hasError
			|| chunkTimeoutField.hasValidationError
			|| provHeadersEditor.hasValidationError
			|| extraProvOptsEditor.hasValidationError
			|| modelsHaveValidationError
		readonly property bool hasPendingChanges: (pendingRenameTarget !== "" && pendingRenameTarget !== providerId)
			|| newModelField.text.trim() !== ""
			|| chunkTimeoutField.hasUncommittedEdit
			|| envVarsEditor.hasUncommittedEdit
			|| provHeadersEditor.hasUncommittedEdit
			|| whitelistEditor.hasUncommittedEdit
			|| blacklistEditor.hasUncommittedEdit
			|| extraProvOptsEditor.hasUncommittedEdit
			|| modelsHavePendingChanges

		function cardMutate(fn) {
			cardMutating = true
			section.mutating = true
			fn()
			section.mutating = false
			cardMutating = false
		}

		function provStr(subPath) {
			const v = section.editor.valueAt(["provider", providerId].concat(subPath))
			return typeof v === "string" ? v : ""
		}

		function provNum(subPath) {
			const v = section.editor.valueAt(["provider", providerId].concat(subPath))
			return typeof v === "number" && Number.isFinite(v) ? v : undefined
		}

		function provList(subPath) {
			const v = section.editor.valueAt(["provider", providerId].concat(subPath))
			return Fields.isArray(v) ? Fields.toArray(v).map(x => String(x ?? "")) : []
		}

		function readBaseUrl() {
			const primary = provStr(["options", "baseURL"])
			if (primary !== "")
				return primary
			const camel = provStr(["options", "baseUrl"])
			if (camel !== "")
				return camel
			return provStr(["options", "base_url"])
		}

		function readApiKey() {
			const primary = provStr(["options", "apiKey"])
			if (primary !== "")
				return primary
			return provStr(["options", "api_key"])
		}

		function pruneEmptyProviderOptions() {
			if (section.editor.keysAt(["provider", providerId, "options"]).length === 0)
				section.editor.removeAt(["provider", providerId, "options"], false)
		}

		function setProviderOptionString(optKey, aliasesToClear, textVal) {
			for (const aliasKey of aliasesToClear)
				section.editor.removeAt(["provider", providerId, "options", aliasKey], false)
			if (textVal === "") {
				section.editor.removeAt(["provider", providerId, "options", optKey], false)
				pruneEmptyProviderOptions()
			} else {
				section.editor.setValueAt(["provider", providerId, "options", optKey], textVal)
			}
		}

		function readExtraProviderOptions() {
			const allOpts = section.editor.orderedEntriesAt(["provider", providerId, "options"])
			return allOpts.filter(item => !section.knownProviderOptionKeys.includes(item.key))
		}

		function writeExtraProviderOptions(extraEntries) {
			cardMutate(() => {
				const existingOpts = section.editor.orderedEntriesAt(["provider", providerId, "options"])
				const extraByKey = {}
				for (const item of extraEntries) {
					if (!section.knownProviderOptionKeys.includes(item.key))
						extraByKey[item.key] = item
				}
				const merged = []
				const seenExtra = {}
				for (const oldItem of existingOpts) {
					if (section.knownProviderOptionKeys.includes(oldItem.key)) {
						merged.push(oldItem)
					} else if (Fields.has(extraByKey, oldItem.key)) {
						merged.push(extraByKey[oldItem.key])
						seenExtra[oldItem.key] = true
					}
				}
				for (const item of extraEntries) {
					if (!section.knownProviderOptionKeys.includes(item.key) && !Fields.has(seenExtra, item.key))
						merged.push(item)
				}
				section.editor.setOrderedObjectAt(["provider", providerId, "options"], merged, true, false)
			})
		}

		function syncProviderCard() {
			const nextModels = section.editor.keysAt(["provider", providerId, "models"])
			if (!section.sameKeys(modelKeys, nextModels))
				modelKeys = nextModels

			provRenameField.text = providerId
			provRenameField.errorText = ""
			provNameField.text = provStr(["name"])
			provApiSelect.value = provStr(["api"])
			provNpmSelect.value = provStr(["npm"])
			baseUrlField.text = readBaseUrl()
			apiKeyField.text = readApiKey()
			envVarsEditor.values = provList(["env"])
			chunkTimeoutField.value = provNum(["options", "chunkTimeout"])
			provHeadersEditor.loadFromEntries(section.editor.orderedEntriesAt(["provider", providerId, "options", "headers"]))
			whitelistEditor.values = provList(["whitelist"])
			blacklistEditor.values = provList(["blacklist"])
			extraProvOptsEditor.loadFromEntries(readExtraProviderOptions())
		}

		function validateProviderRenameLive() {
			const target = provRenameField.text.trim()
			if (target !== "" && target !== providerId && section.editor.keysAt(["provider"]).includes(target))
				provRenameField.errorText = qsTr("Provider \"%1\" already exists.").arg(target)
			else
				provRenameField.errorText = ""
		}

		function commitProviderRename() {
			const target = provRenameField.text.trim()
			if (target === "" || target === providerId) {
				provRenameField.text = providerId
				provRenameField.errorText = ""
				return true
			}
			if (section.editor.keysAt(["provider"]).includes(target)) {
				provRenameField.errorText = qsTr("Provider \"%1\" already exists.").arg(target)
				return false
			}
			provRenameField.errorText = ""
			section.renameProviderExpanded(providerId, target)
			section.editor.renameKeyAt(["provider"], providerId, target)
			return true
		}

		function addModel() {
			const mKey = newModelField.text.trim()
			if (mKey === "") {
				newModelField.errorText = qsTr("Enter a model key.")
				return false
			}
			if (section.editor.keysAt(["provider", providerId, "models"]).includes(mKey)) {
				newModelField.errorText = qsTr("Model \"%1\" already exists.").arg(mKey)
				return false
			}
			newModelField.errorText = ""
			newModelField.text = ""
			section.setModelExpanded(providerId, mKey, true)
			section.editor.setValueAt(["provider", providerId, "models", mKey], {})
			return true
		}

		function commitPending() {
			const trimmedName = provNameField.text.trim()
			if (provNameField.text !== trimmedName) {
				provNameField.text = trimmedName
				cardMutate(() => {
					if (trimmedName === "")
						section.editor.removeAt(["provider", providerId, "name"], false)
					else
						section.editor.setValueAt(["provider", providerId, "name"], trimmedName)
				})
			}
			if (newModelField.text.trim() !== "" && !addModel())
				return false
			if (!envVarsEditor.commitPending()
				|| !provHeadersEditor.commitPending()
				|| !whitelistEditor.commitPending()
				|| !blacklistEditor.commitPending()
				|| !extraProvOptsEditor.commitPending())
				return false
			for (let i = 0; i < modelRepeater.count; ++i) {
				const m = modelRepeater.itemAt(i) as ModelCard
				if (m && !m.commitPending())
					return false
			}
			if (hasValidationError)
				return false
			return commitProviderRename()
		}

		function resetCardState() {
			apiKeyField.revealed = false
			newModelField.text = ""
			newModelField.errorText = ""
			chunkTimeoutField.resetState()
			envVarsEditor.resetState()
			provHeadersEditor.resetState()
			whitelistEditor.resetState()
			blacklistEditor.resetState()
			extraProvOptsEditor.resetState()
			for (let i = 0; i < modelRepeater.count; ++i) {
				const m = modelRepeater.itemAt(i) as ModelCard
				if (m)
					m.resetModelState()
			}
			syncProviderCard()
		}

		width: section.width
		implicitHeight: provHeader.height + (expanded ? provBody.implicitHeight + 16 : 0)
		radius: Theme.radius
		color: Theme.background
		border.color: Theme.border
		clip: true

		Component.onCompleted: syncProviderCard()

		Connections {
			target: section.editor
			function onParsedChanged() {
				if (!provCard.cardMutating)
					provCard.syncProviderCard()
				else {
					const nextModels = section.editor.keysAt(["provider", provCard.providerId, "models"])
					if (!section.sameKeys(provCard.modelKeys, nextModels))
						provCard.modelKeys = nextModels
				}
			}
		}

		Item {
			id: provHeader
			width: parent.width
			height: 46

			Item {
				anchors { left: parent.left; right: deleteProvBtn.left; top: parent.top; bottom: parent.bottom }

				Chevron {
					id: pChev
					x: 14
					anchors.verticalCenter: parent.verticalCenter
					rotation: provCard.expanded ? 90 : 0
					color: Theme.textMuted
					Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				Text {
					id: pIdLabel
					textFormat: Text.PlainText
					anchors { left: pChev.right; leftMargin: 10; verticalCenter: parent.verticalCenter }
					text: provCard.providerId
					color: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize; bold: true }
				}

				Rectangle {
					id: modelCountPill
					anchors { left: pIdLabel.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
					width: modelCountText.implicitWidth + 12
					height: 20
					radius: 4
					color: Theme.codeBackground
					border.color: Theme.border

					Text {
						id: modelCountText
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: qsTr("%1 models").arg(provCard.modelKeys.length)
						color: Theme.textMuted
						font.pixelSize: Theme.microFontSize
					}
				}

				Text {
					textFormat: Text.PlainText
					anchors { left: modelCountPill.right; leftMargin: 10; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
					text: {
						void section.editor.revision
						const disp = provCard.provStr(["name"])
						const url = provCard.readBaseUrl()
						if (disp !== "" && url !== "")
							return disp + " · " + url
						return disp !== "" ? disp : url
					}
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: section.setProviderExpanded(provCard.providerId, !provCard.expanded) }
			}

			IconButton {
				id: deleteProvBtn
				anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
				buttonSize: 28
				iconName: "trash"
				iconSize: 16
				tooltipText: qsTr("Delete provider")
				Accessible.name: qsTr("Delete provider")
				onClicked: section.editor.removeAt(["provider", provCard.providerId])
			}
		}

		Column {
			id: provBody
			visible: provCard.expanded
			y: provHeader.height
			x: 14
			width: parent.width - 28
			spacing: 4

			Rectangle { width: parent.width; height: 1; color: Theme.border }

			FormRow {
				label: qsTr("Provider ID")
				description: qsTr("Used as the prefix in provider/model references.")
				showDivider: true

				Row {
					width: parent.width
					spacing: 6

					DialogField {
						id: provRenameField
						objectName: "providerRenameField_" + provCard.providerId
						width: parent.width - provRenameBtn.width - 6
						placeholderText: qsTr("Provider ID")
						onEdited: provCard.validateProviderRenameLive()
						onAccepted: provCard.commitProviderRename()
						onEditingFinished: provCard.commitProviderRename()
					}

					DialogButton {
						id: provRenameBtn
						objectName: "providerRenameBtn_" + provCard.providerId
						text: qsTr("Rename")
						enabled: provCard.pendingRenameTarget !== ""
							&& provCard.pendingRenameTarget !== provCard.providerId
							&& !provRenameField.hasError
						onClicked: provCard.commitProviderRename()
					}
				}
			}

			FormRow {
				label: qsTr("Display name")
				badge: "name"
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "name"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "name"], false)
				}

				DialogField {
					id: provNameField
					width: parent.width
					placeholderText: provCard.providerId
					onEdited: provCard.cardMutate(() => {
						if (text.trim() === "")
							section.editor.removeAt(["provider", provCard.providerId, "name"], false)
						else
							section.editor.setValueAt(["provider", provCard.providerId, "name"], text)
					})
					onEditingFinished: {
						const trimmed = text.trim()
						if (text !== trimmed)
							text = trimmed
						provCard.cardMutate(() => {
							if (trimmed === "")
								section.editor.removeAt(["provider", provCard.providerId, "name"], false)
							else
								section.editor.setValueAt(["provider", provCard.providerId, "name"], trimmed)
						})
					}
				}
			}

			FormRow {
				label: qsTr("API protocol")
				badge: "api"
				description: qsTr("chat, responses, anthropic, or an http(s):// base URL.")
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "api"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "api"], false)
				}

				SelectField {
					id: provApiSelect
					width: parent.width
					editable: true
					clearable: true
					options: [
						{ label: qsTr("Auto (from npm / Chat)"), value: "" },
						{ label: "chat", description: qsTr("OpenAI Chat Completions"), value: "chat" },
						{ label: "responses", description: qsTr("OpenAI Responses"), value: "responses" },
						{ label: "anthropic", description: qsTr("Anthropic Messages"), value: "anthropic" }
					]
					placeholderText: qsTr("Auto (OpenAI Chat)")
					onActivated: val => provCard.cardMutate(() => {
						if (val === "")
							section.editor.removeAt(["provider", provCard.providerId, "api"], false)
						else
							section.editor.setValueAt(["provider", provCard.providerId, "api"], val)
					})
				}
			}

			FormRow {
				label: qsTr("SDK package")
				badge: "npm"
				description: qsTr("opencode SDK selector when api is unset.")
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "npm"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "npm"], false)
				}

				SelectField {
					id: provNpmSelect
					width: parent.width
					editable: true
					clearable: true
					options: [
						"@ai-sdk/openai-compatible",
						"@ai-sdk/anthropic",
						"@ai-sdk/openai",
						"@ai-sdk/azure",
						"@ai-sdk/google-vertex/anthropic"
					]
					placeholderText: "@ai-sdk/openai-compatible"
					onActivated: val => provCard.cardMutate(() => {
						if (val === "")
							section.editor.removeAt(["provider", provCard.providerId, "npm"], false)
						else
							section.editor.setValueAt(["provider", provCard.providerId, "npm"], val)
					})
				}
			}

			FormRow {
				label: qsTr("Base URL")
				badge: "options.baseURL"
				showDivider: true
				showReset: {
					void section.editor.revision
					return section.editor.contains(["provider", provCard.providerId, "options", "baseURL"])
						|| section.editor.contains(["provider", provCard.providerId, "options", "baseUrl"])
						|| section.editor.contains(["provider", provCard.providerId, "options", "base_url"])
				}
				onResetClicked: {
					section.forceActiveFocus()
					provCard.cardMutate(() => provCard.setProviderOptionString("baseURL", ["baseUrl", "base_url"], ""))
					baseUrlField.text = ""
				}

				DialogField {
					id: baseUrlField
					objectName: "providerBaseUrl_" + provCard.providerId
					width: parent.width
					placeholderText: "https://api.example.com/v1"
					onEdited: provCard.cardMutate(() => provCard.setProviderOptionString("baseURL", ["baseUrl", "base_url"], text.trim()))
					onEditingFinished: {
						const trimmed = text.trim()
						if (text !== trimmed)
							text = trimmed
						provCard.cardMutate(() => provCard.setProviderOptionString("baseURL", ["baseUrl", "base_url"], trimmed))
					}
				}
			}

			FormRow {
				label: qsTr("API key")
				badge: "options.apiKey"
				description: qsTr("Literal key, {env:VAR}, or {file:…} secret path.")
				showDivider: true
				showReset: {
					void section.editor.revision
					return section.editor.contains(["provider", provCard.providerId, "options", "apiKey"])
						|| section.editor.contains(["provider", provCard.providerId, "options", "api_key"])
				}
				onResetClicked: {
					section.forceActiveFocus()
					apiKeyField.revealed = false
					provCard.cardMutate(() => provCard.setProviderOptionString("apiKey", ["api_key"], ""))
					apiKeyField.text = ""
				}

				SecretField {
					id: apiKeyField
					objectName: "providerApiKey_" + provCard.providerId
					width: parent.width
					envStatus: section.editor.envStatus
					placeholderText: "{env:API_KEY}"
					onEdited: provCard.cardMutate(() => provCard.setProviderOptionString("apiKey", ["api_key"], text))
				}
			}

			FormRow {
				label: qsTr("Fallback env variables")
				badge: "env"
				description: qsTr("Checked in order when options.apiKey is unset.")
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "env"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "env"], false)
					envVarsEditor.resetState()
				}

				StringListEditor {
					id: envVarsEditor
					width: parent.width
					monospace: true
					placeholderText: "MY_API_KEY"
					emptyText: qsTr("Default provider env var")
					onValuesModified: vals => provCard.cardMutate(() => {
						if (vals.length === 0)
							section.editor.removeAt(["provider", provCard.providerId, "env"], false)
						else
							section.editor.setStringListAt(["provider", provCard.providerId, "env"], vals, false)
					})
				}
			}

			FormRow {
				label: qsTr("Send prompt cache key")
				badge: "options.setCacheKey"
				controlWidth: 0
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "options", "setCacheKey"]) }
				onResetClicked: {
					section.forceActiveFocus()
					provCard.cardMutate(() => {
						section.editor.removeAt(["provider", provCard.providerId, "options", "setCacheKey"], false)
						provCard.pruneEmptyProviderOptions()
					})
				}

				Switch {
					accessibleName: qsTr("Send prompt cache key")
					checked: {
						void section.editor.revision
						const v = section.editor.valueAt(["provider", provCard.providerId, "options", "setCacheKey"])
						return typeof v === "boolean" ? v : false
					}
					onToggled: val => provCard.cardMutate(() => section.editor.setValueAt(["provider", provCard.providerId, "options", "setCacheKey"], val))
				}
			}

			FormRow {
				label: qsTr("Stream chunk timeout")
				badge: "options.chunkTimeout"
				description: qsTr("Milliseconds before an idle stream is retried (default 300000).")
				controlWidth: 200
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "options", "chunkTimeout"]) }
				onResetClicked: {
					section.forceActiveFocus()
					provCard.cardMutate(() => {
						section.editor.removeAt(["provider", provCard.providerId, "options", "chunkTimeout"], false)
						provCard.pruneEmptyProviderOptions()
					})
					chunkTimeoutField.value = undefined
					chunkTimeoutField.resetState()
				}

				NumberField {
					id: chunkTimeoutField
					objectName: "providerChunkTimeout_" + provCard.providerId
					width: parent.width
					minimum: 0
					step: 5000
					suffix: "ms"
					placeholderText: "300000"
					onValueModified: val => provCard.cardMutate(() => {
						if (val === undefined) {
							section.editor.removeAt(["provider", provCard.providerId, "options", "chunkTimeout"], false)
							provCard.pruneEmptyProviderOptions()
						} else {
							section.editor.setValueAt(["provider", provCard.providerId, "options", "chunkTimeout"], val)
						}
					})
				}
			}

			FormRow {
				label: qsTr("HTTP headers")
				badge: "options.headers"
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "options", "headers"]) }
				onResetClicked: {
					section.forceActiveFocus()
					provCard.cardMutate(() => {
						section.editor.removeAt(["provider", provCard.providerId, "options", "headers"], false)
						provCard.pruneEmptyProviderOptions()
					})
					provHeadersEditor.loadFromEntries([])
					provHeadersEditor.resetState()
				}

				KeyValueEditor {
					id: provHeadersEditor
					objectName: "providerHeadersEditor_" + provCard.providerId
					width: parent.width
					valueMode: "string"
					envStatus: section.editor.envStatus
					keyPlaceholder: "X-Org"
					valuePlaceholder: "{env:ORG_ID}"
					emptyText: qsTr("No custom provider headers")
					onEntriesModified: nextEntries => provCard.cardMutate(() => {
						section.editor.setOrderedObjectAt(["provider", provCard.providerId, "options", "headers"], nextEntries, true, false)
						provCard.pruneEmptyProviderOptions()
					})
				}
			}

			FormRow {
				label: qsTr("Model whitelist")
				badge: "whitelist"
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "whitelist"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "whitelist"], false)
					whitelistEditor.resetState()
				}

				StringListEditor {
					id: whitelistEditor
					width: parent.width
					monospace: true
					placeholderText: qsTr("Allowed model key")
					emptyText: qsTr("All models allowed")
					onValuesModified: vals => provCard.cardMutate(() => {
						if (vals.length === 0)
							section.editor.removeAt(["provider", provCard.providerId, "whitelist"], false)
						else
							section.editor.setStringListAt(["provider", provCard.providerId, "whitelist"], vals, false)
					})
				}
			}

			FormRow {
				label: qsTr("Model blacklist")
				badge: "blacklist"
				stacked: true
				showDivider: true
				showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "blacklist"]) }
				onResetClicked: {
					section.forceActiveFocus()
					section.editor.removeAt(["provider", provCard.providerId, "blacklist"], false)
					blacklistEditor.resetState()
				}

				StringListEditor {
					id: blacklistEditor
					width: parent.width
					monospace: true
					placeholderText: qsTr("Excluded model key")
					emptyText: qsTr("No models excluded")
					onValuesModified: vals => provCard.cardMutate(() => {
						if (vals.length === 0)
							section.editor.removeAt(["provider", provCard.providerId, "blacklist"], false)
						else
							section.editor.setStringListAt(["provider", provCard.providerId, "blacklist"], vals, false)
					})
				}
			}

			FormRow {
				label: qsTr("Extra provider options")
				badge: "options.*"
				description: qsTr("Additional vendor-specific fields under provider.<id>.options.")
				stacked: true
				showDivider: true

				KeyValueEditor {
					id: extraProvOptsEditor
					objectName: "extraProviderOptionsEditor_" + provCard.providerId
					width: parent.width
					valueMode: "json"
					keyPlaceholder: qsTr("Option key")
					valuePlaceholder: qsTr("JSON value")
					emptyText: qsTr("No extra options")
					onEntriesModified: nextEntries => provCard.writeExtraProviderOptions(nextEntries)
				}
			}

			// ---- Nested Models CRUD ----
			Text {
				textFormat: Text.PlainText
				width: parent.width
				topPadding: 10
				text: qsTr("Models (%1)").arg(provCard.modelKeys.length)
				color: Theme.text
				font { pixelSize: Theme.secondaryFontSize; bold: true }
			}

			Item {
				width: parent.width
				implicitHeight: newModelField.implicitHeight

				DialogField {
					id: newModelField
					objectName: "newModelField_" + provCard.providerId
					anchors { left: parent.left; right: addModelBtn.left; rightMargin: 8; top: parent.top }
					placeholderText: qsTr("Model key (e.g. gpt-5, qwen3-coder)")
					onEdited: errorText = ""
					onAccepted: provCard.addModel()
				}

				DialogButton {
					id: addModelBtn
					objectName: "addModelButton_" + provCard.providerId
					anchors { right: parent.right; top: parent.top }
					kind: DialogButton.Primary
					text: qsTr("+ Add Model")
					onClicked: provCard.addModel()
				}
			}

			Column {
				width: parent.width
				spacing: 8

				Repeater {
					id: modelRepeater
					model: provCard.modelKeys
					onItemAdded: provCard.modelsRevision++
					onItemRemoved: provCard.modelsRevision++

					delegate: ModelCard {
						providerId: provCard.providerId
						onBeginParentMutate: {
							provCard.cardMutating = true
							section.mutating = true
						}
						onEndParentMutate: {
							section.mutating = false
							provCard.cardMutating = false
						}
					}
				}
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Providers & Models")
		color: Theme.text
		font { pixelSize: Theme.fontSize; bold: true }
	}

	Text {
		textFormat: Text.PlainText
		width: parent.width
		text: qsTr("Add a built-in provider preset or any OpenAI / Anthropic compatible endpoint, then configure models and variants.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Flow {
		width: parent.width
		spacing: 6

		Repeater {
			model: section.presets

			delegate: DialogButton {
				required property var modelData
				objectName: "presetProvider_" + modelData.id
				small: true
				text: "+ " + modelData.label
				onClicked: section.triggerPreset(modelData.id)
			}
		}

		DialogButton {
			objectName: "toggleCustomProviderButton"
			small: true
			kind: section.showingCustomInput ? DialogButton.Secondary : DialogButton.Primary
			text: qsTr("+ Custom Provider…")
			onClicked: {
				section.showingCustomInput = !section.showingCustomInput
				if (section.showingCustomInput)
					customProviderIdField.inputItem.forceActiveFocus()
			}
		}
	}

	Item {
		visible: section.showingCustomInput
		width: parent.width
		implicitHeight: customProviderIdField.implicitHeight

		DialogField {
			id: customProviderIdField
			objectName: "customProviderIdField"
			anchors { left: parent.left; right: customBtns.left; rightMargin: 8; top: parent.top }
			placeholderText: qsTr("Provider ID (e.g. vllm, together, azure)")
			onEdited: errorText = ""
			onAccepted: section.addCustomProvider()
		}

		Row {
			id: customBtns
			anchors { right: parent.right; top: parent.top }
			spacing: 6

			DialogButton {
				objectName: "confirmCustomProviderButton"
				kind: DialogButton.Primary
				text: qsTr("Add")
				onClicked: section.addCustomProvider()
			}

			DialogButton {
				text: qsTr("Cancel")
				onClicked: {
					section.showingCustomInput = false
					customProviderIdField.errorText = ""
					customProviderIdField.text = ""
				}
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: section.providerKeys.length === 0
		width: parent.width
		topPadding: 4
		text: qsTr("No providers explicitly configured in this file (environment-detected defaults still apply).")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Repeater {
		id: provRepeater
		model: section.providerKeys
		onItemAdded: section.cardsRevision++
		onItemRemoved: section.cardsRevision++

		delegate: ProvCard {}
	}
}
