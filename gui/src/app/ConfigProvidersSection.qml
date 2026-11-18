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

	readonly property var presets: [
		{ id: "anthropic", label: "Anthropic" },
		{ id: "openai", label: "OpenAI" },
		{ id: "deepseek", label: "DeepSeek" },
		{ id: "openrouter", label: "OpenRouter" },
		{ id: "ollama", label: qsTr("Ollama / Local") }
	]
	readonly property var knownProviderOptionKeys: ["baseURL", "apiKey", "headers", "setCacheKey", "chunkTimeout"]

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

	function isModelExpanded(providerId, modelKey) {
		return Boolean(expandedModels[providerId + "/" + modelKey])
	}

	function setModelExpanded(providerId, modelKey, open) {
		const next = Object.assign({}, expandedModels)
		next[providerId + "/" + modelKey] = open
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
			return
		}
		const created = editor.addProviderPreset("custom", requested)
		if (created === "") {
			customProviderIdField.errorText = qsTr("Provider \"%1\" already exists.").arg(requested)
			return
		}
		customProviderIdField.errorText = ""
		customProviderIdField.text = ""
		showingCustomInput = false
		setProviderExpanded(created, true)
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
		model: section.providerKeys

		delegate: Rectangle {
			id: provCard
			required property string modelData
			readonly property string providerId: modelData
			readonly property bool expanded: section.isProviderExpanded(providerId)
			property bool cardMutating: false
			property var modelKeys: []

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
				return Array.isArray(v) ? v.map(x => String(x ?? "")) : []
			}

			function readExtraProviderOptions() {
				const opts = section.editor.valueAt(["provider", providerId, "options"])
				if (!opts || typeof opts !== "object" || Array.isArray(opts))
					return {}
				const extra = {}
				for (const k of Object.keys(opts)) {
					if (Fields.has(opts, k) && !section.knownProviderOptionKeys.includes(k))
						extra[k] = opts[k]
				}
				return extra
			}

			function writeExtraProviderOptions(newMap) {
				cardMutate(() => {
					const opts = section.editor.valueAt(["provider", providerId, "options"])
					if (opts && typeof opts === "object" && !Array.isArray(opts)) {
						for (const oldKey of Object.keys(opts)) {
							if (!section.knownProviderOptionKeys.includes(oldKey) && !Fields.has(newMap, oldKey))
								section.editor.removeAt(["provider", providerId, "options", oldKey], true)
						}
					}
					for (const newKey of Object.keys(newMap)) {
						if (!section.knownProviderOptionKeys.includes(newKey))
							section.editor.setValueAt(["provider", providerId, "options", newKey], newMap[newKey])
					}
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
				baseUrlField.text = provStr(["options", "baseURL"])
				apiKeyField.text = provStr(["options", "apiKey"])
				envVarsEditor.values = provList(["env"])
				chunkTimeoutField.value = provNum(["options", "chunkTimeout"])

				const hdrs = section.editor.valueAt(["provider", providerId, "options", "headers"])
				provHeadersEditor.loadFromMap(hdrs && typeof hdrs === "object" && !Array.isArray(hdrs) ? hdrs : {})

				whitelistEditor.values = provList(["whitelist"])
				blacklistEditor.values = provList(["blacklist"])
				extraProvOptsEditor.loadFromMap(readExtraProviderOptions())
			}

			function commitProviderRename() {
				const target = provRenameField.text.trim()
				if (target === "" || target === providerId) {
					provRenameField.text = providerId
					provRenameField.errorText = ""
					return
				}
				if (!section.editor.renameKeyAt(["provider"], providerId, target)) {
					provRenameField.errorText = qsTr("Provider \"%1\" already exists.").arg(target)
					return
				}
				provRenameField.errorText = ""
				section.setProviderExpanded(target, true)
			}

			function addModel() {
				const mKey = newModelField.text.trim()
				if (mKey === "") {
					newModelField.errorText = qsTr("Enter a model key.")
					return
				}
				if (section.editor.keysAt(["provider", providerId, "models"]).includes(mKey)) {
					newModelField.errorText = qsTr("Model \"%1\" already exists.").arg(mKey)
					return
				}
				newModelField.errorText = ""
				newModelField.text = ""
				section.setModelExpanded(providerId, mKey, true)
				section.editor.setValueAt(["provider", providerId, "models", mKey], {})
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
							const url = provCard.provStr(["options", "baseURL"])
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
							width: parent.width - provRenameBtn.width - 6
							placeholderText: qsTr("Provider ID")
							onEdited: errorText = ""
							onAccepted: provCard.commitProviderRename()
						}

						DialogButton {
							id: provRenameBtn
							text: qsTr("Rename")
							enabled: provRenameField.text.trim() !== "" && provRenameField.text.trim() !== provCard.providerId
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
						onEdited: provCard.cardMutate(() => section.editor.setStringAt(["provider", provCard.providerId, "name"], text))
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
						onActivated: val => provCard.cardMutate(() => section.editor.setStringAt(["provider", provCard.providerId, "api"], val))
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
						onActivated: val => provCard.cardMutate(() => section.editor.setStringAt(["provider", provCard.providerId, "npm"], val))
					}
				}

				FormRow {
					label: qsTr("Base URL")
					badge: "options.baseURL"
					showDivider: true
					showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "options", "baseURL"]) }
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["provider", provCard.providerId, "options", "baseURL"], false)
					}

					DialogField {
						id: baseUrlField
						objectName: "providerBaseUrl_" + provCard.providerId
						width: parent.width
						placeholderText: "https://api.example.com/v1"
						onEdited: provCard.cardMutate(() => section.editor.setStringAt(["provider", provCard.providerId, "options", "baseURL"], text))
					}
				}

				FormRow {
					label: qsTr("API key")
					badge: "options.apiKey"
					description: qsTr("Literal key, {env:VAR}, or {file:…} secret path.")
					showDivider: true
					showReset: { void section.editor.revision; return section.editor.contains(["provider", provCard.providerId, "options", "apiKey"]) }
					onResetClicked: {
						section.forceActiveFocus()
						section.editor.removeAt(["provider", provCard.providerId, "options", "apiKey"], false)
					}

					SecretField {
						id: apiKeyField
						objectName: "providerApiKey_" + provCard.providerId
						width: parent.width
						envStatus: section.editor.envStatus
						placeholderText: "{env:API_KEY}"
						onEdited: provCard.cardMutate(() => section.editor.setStringAt(["provider", provCard.providerId, "options", "apiKey"], text))
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
					}

					StringListEditor {
						id: envVarsEditor
						width: parent.width
						monospace: true
						placeholderText: "MY_API_KEY"
						emptyText: qsTr("Default provider env var")
						onValuesModified: vals => provCard.cardMutate(() => section.editor.setStringListAt(["provider", provCard.providerId, "env"], vals))
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
						section.editor.removeAt(["provider", provCard.providerId, "options", "setCacheKey"], false)
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
						section.editor.removeAt(["provider", provCard.providerId, "options", "chunkTimeout"], false)
					}

					NumberField {
						id: chunkTimeoutField
						width: parent.width
						minimum: 0
						step: 5000
						suffix: "ms"
						placeholderText: "300000"
						onValueModified: val => provCard.cardMutate(() => section.editor.setValueAt(["provider", provCard.providerId, "options", "chunkTimeout"], val))
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
						section.editor.removeAt(["provider", provCard.providerId, "options", "headers"], false)
					}

					KeyValueEditor {
						id: provHeadersEditor
						width: parent.width
						valueMode: "string"
						envStatus: section.editor.envStatus
						keyPlaceholder: "X-Org"
						valuePlaceholder: "{env:ORG_ID}"
						emptyText: qsTr("No custom provider headers")
						onMapModified: newMap => provCard.cardMutate(() => {
							if (Object.keys(newMap).length === 0)
								section.editor.removeAt(["provider", provCard.providerId, "options", "headers"], false)
							else
								section.editor.setValueAt(["provider", provCard.providerId, "options", "headers"], newMap)
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
					}

					StringListEditor {
						id: whitelistEditor
						width: parent.width
						monospace: true
						placeholderText: qsTr("Allowed model key")
						emptyText: qsTr("All models allowed")
						onValuesModified: vals => provCard.cardMutate(() => section.editor.setStringListAt(["provider", provCard.providerId, "whitelist"], vals))
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
					}

					StringListEditor {
						id: blacklistEditor
						width: parent.width
						monospace: true
						placeholderText: qsTr("Excluded model key")
						emptyText: qsTr("No models excluded")
						onValuesModified: vals => provCard.cardMutate(() => section.editor.setStringListAt(["provider", provCard.providerId, "blacklist"], vals))
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
						width: parent.width
						valueMode: "json"
						keyPlaceholder: qsTr("Option key")
						valuePlaceholder: qsTr("JSON value")
						emptyText: qsTr("No extra options")
						onMapModified: newMap => provCard.writeExtraProviderOptions(newMap)
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
						model: provCard.modelKeys

						delegate: Rectangle {
							id: modelCard
							required property string modelData
							readonly property string modelKey: modelData
							readonly property bool mExpanded: section.isModelExpanded(provCard.providerId, modelKey)
							property bool modelMutating: false
							property var variantKeys: []

							function mPath(tail) {
								return ["provider", provCard.providerId, "models", modelKey].concat(tail)
							}

							function mMutate(fn) {
								modelMutating = true
								provCard.cardMutate(fn)
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

								const mHdrs = section.editor.valueAt(mPath(["headers"]))
								mHeadersEditor.loadFromMap(mHdrs && typeof mHdrs === "object" && !Array.isArray(mHdrs) ? mHdrs : {})

								const mOpts = section.editor.valueAt(mPath(["options"]))
								mOptionsEditor.loadFromMap(mOpts && typeof mOpts === "object" && !Array.isArray(mOpts) ? mOpts : {})
							}

							function commitModelRename() {
								const target = modelRenameField.text.trim()
								if (target === "" || target === modelKey) {
									modelRenameField.text = modelKey
									modelRenameField.errorText = ""
									return
								}
								if (!section.editor.renameKeyAt(["provider", provCard.providerId, "models"], modelKey, target)) {
									modelRenameField.errorText = qsTr("Model \"%1\" already exists.").arg(target)
									return
								}
								modelRenameField.errorText = ""
								section.setModelExpanded(provCard.providerId, target, true)
							}

							function addVariant() {
								const vKey = newVariantField.text.trim()
								if (vKey === "") {
									newVariantField.errorText = qsTr("Enter a variant name.")
									return
								}
								if (section.editor.keysAt(mPath(["variants"])).includes(vKey)) {
									newVariantField.errorText = qsTr("Variant \"%1\" already exists.").arg(vKey)
									return
								}
								newVariantField.errorText = ""
								newVariantField.text = ""
								section.editor.setValueAt(mPath(["variants", vKey]), {})
							}

							width: parent.width
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
									TapHandler { onTapped: section.setModelExpanded(provCard.providerId, modelCard.modelKey, !modelCard.mExpanded) }
								}

								IconButton {
									id: delModelBtn
									anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
									buttonSize: 26
									iconName: "trash"
									iconSize: 16
									tooltipText: qsTr("Delete model")
									Accessible.name: qsTr("Delete model")
									onClicked: section.editor.removeAt(["provider", provCard.providerId, "models", modelCard.modelKey], false)
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
											width: parent.width - mRenameBtn.width - 6
											placeholderText: qsTr("Model key")
											onEdited: errorText = ""
											onAccepted: modelCard.commitModelRename()
										}

										DialogButton {
											id: mRenameBtn
											text: qsTr("Rename")
											enabled: modelRenameField.text.trim() !== "" && modelRenameField.text.trim() !== modelCard.modelKey
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
										onEdited: modelCard.mMutate(() => section.editor.setStringAt(modelCard.mPath(["id"]), text))
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
										onEdited: modelCard.mMutate(() => section.editor.setStringAt(modelCard.mPath(["name"]), text))
									}
								}

								FormRow {
									label: qsTr("Accepts image attachments")
									badge: "attachment"
									controlWidth: 0
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["attachment"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["attachment"]), false) }

									Switch {
										accessibleName: qsTr("Accepts image attachments")
										checked: { void section.editor.revision; return modelCard.mBool(["attachment"], false) }
										onToggled: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["attachment"]), val))
									}
								}

								FormRow {
									label: qsTr("Reasoning model")
									badge: "reasoning"
									controlWidth: 0
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["reasoning"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["reasoning"]), false) }

									Switch {
										accessibleName: qsTr("Reasoning model")
										checked: { void section.editor.revision; return modelCard.mBool(["reasoning"], false) }
										onToggled: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["reasoning"]), val))
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
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["limit", "context"]), true) }

									NumberField {
										id: ctxLimitField
										width: parent.width
										minimum: 1
										step: 8192
										placeholderText: "128000"
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["limit", "context"]), val))
									}
								}

								FormRow {
									label: qsTr("Max output tokens")
									badge: "limit.output"
									controlWidth: 200
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "output"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["limit", "output"]), true) }

									NumberField {
										id: outLimitField
										width: parent.width
										minimum: 1
										step: 4096
										placeholderText: "32000"
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["limit", "output"]), val))
									}
								}

								FormRow {
									label: qsTr("Recommended compaction tokens")
									badge: "limit.compact"
									controlWidth: 200
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "compact"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["limit", "compact"]), true) }

									NumberField {
										id: compactLimitField
										width: parent.width
										minimum: 1
										step: 8192
										placeholderText: qsTr("Global default")
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["limit", "compact"]), val))
									}
								}

								FormRow {
									label: qsTr("Auto-compaction threshold")
									badge: "limit.threshold"
									controlWidth: 200
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["limit", "threshold"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["limit", "threshold"]), true) }

									NumberField {
										id: threshLimitField
										width: parent.width
										decimals: true
										minimum: 0
										step: 0.05
										placeholderText: qsTr("Global default")
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["limit", "threshold"]), val))
									}
								}

								FormRow {
									label: qsTr("Temperature")
									badge: "temperature"
									controlWidth: 180
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["temperature"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["temperature"]), false) }

									NumberField {
										id: mTempField
										width: parent.width
										decimals: true
										minimum: 0
										maximum: 2
										step: 0.1
										placeholderText: qsTr("Provider default")
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["temperature"]), val))
									}
								}

								FormRow {
									label: qsTr("Top-p")
									badge: "top_p"
									controlWidth: 180
									showDivider: true
									showReset: { void section.editor.revision; return section.editor.contains(modelCard.mPath(["top_p"])) }
									onResetClicked: { section.forceActiveFocus(); section.editor.removeAt(modelCard.mPath(["top_p"]), false) }

									NumberField {
										id: mTopPField
										width: parent.width
										decimals: true
										minimum: 0
										maximum: 1
										step: 0.05
										placeholderText: qsTr("Provider default")
										onValueModified: val => modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["top_p"]), val))
									}
								}

								FormRow {
									label: qsTr("Model HTTP headers")
									badge: "headers"
									stacked: true
									showDivider: true

									KeyValueEditor {
										id: mHeadersEditor
										width: parent.width
										valueMode: "string"
										envStatus: section.editor.envStatus
										keyPlaceholder: "X-Model"
										valuePlaceholder: "1"
										emptyText: qsTr("No per-model headers")
										onMapModified: newMap => modelCard.mMutate(() => {
											if (Object.keys(newMap).length === 0)
												section.editor.removeAt(modelCard.mPath(["headers"]), false)
											else
												section.editor.setValueAt(modelCard.mPath(["headers"]), newMap)
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
										width: parent.width
										valueMode: "json"
										keyPlaceholder: "reasoningEffort"
										valuePlaceholder: "\"medium\""
										emptyText: qsTr("No model request options")
										onMapModified: newMap => modelCard.mMutate(() => {
											if (Object.keys(newMap).length === 0)
												section.editor.removeAt(modelCard.mPath(["options"]), false)
											else
												section.editor.setValueAt(modelCard.mPath(["options"]), newMap)
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
											model: modelCard.variantKeys

											delegate: Rectangle {
												id: varBox
												required property string modelData
												readonly property string variantKey: modelData
												property bool varMutating: false

												function syncVariant() {
													const obj = section.editor.valueAt(modelCard.mPath(["variants", variantKey]))
													varOptsEditor.loadFromMap(obj && typeof obj === "object" && !Array.isArray(obj) ? obj : {})
												}

												width: parent.width
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
															onClicked: section.editor.removeAt(modelCard.mPath(["variants", varBox.variantKey]), true)
														}
													}

													KeyValueEditor {
														id: varOptsEditor
														width: parent.width
														valueMode: "json"
														keyPlaceholder: "reasoningEffort"
														valuePlaceholder: "\"high\""
														emptyText: qsTr("No variant fields")
														onMapModified: newMap => {
															varBox.varMutating = true
															modelCard.mMutate(() => section.editor.setValueAt(modelCard.mPath(["variants", varBox.variantKey]), newMap))
															varBox.varMutating = false
														}
													}
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
					}
				}
			}
		}
	}
}
