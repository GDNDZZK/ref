pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// Full graphical and raw JSONC editor for ref.json and ref.json.<n>.bak profiles.
FocusScope {
	id: dialog
	objectName: "configDialog"

	readonly property ConfigEditorController editor: Workspace.configEditor
	readonly property bool open: Workspace.dialog !== ""
	property bool entered: false
	property string selectedCategory: "general"
	property bool creatingNew: false
	property bool confirmingDelete: false
	// `{ type: "close" | "load" | "new" | "activate" | "reload", file?: string }` or null
	property var pendingDirtyAction: null
	property var continueAfterSave: null
	property bool rawMutating: false

	readonly property var categories: [
		{ id: "general", title: qsTr("General"), icon: "file-text" },
		{ id: "providers", title: qsTr("Providers & Models"), icon: "model" },
		{ id: "agents", title: qsTr("Agents"), icon: "agent" },
		{ id: "permissions", title: qsTr("Permissions & Delays"), icon: "lock" },
		{ id: "mcp", title: qsTr("MCP Servers"), icon: "mcp" },
		{ id: "commands", title: qsTr("Commands & Context"), icon: "slash" }
	]

	readonly property var profileOptions: editor.files.map(entry => {
		const file = String(entry.file ?? "")
		const label = String(entry.label ?? "")
		const summary = String(entry.summary ?? "")
		const active = Boolean(entry.active)
		const prefix = active ? "★ " : ""
		const head = label !== "" ? (prefix + label + " (" + file + ")") : (prefix + file)
		return {
			label: head,
			value: file,
			description: summary
		}
	})

	readonly property int lineCount: {
		const txt = rawEditor.text
		if (txt === "")
			return 1
		return txt.split("\n").length
	}

	readonly property string lineNumbersText: {
		const count = Math.max(1, lineCount)
		const lines = []
		for (let i = 1; i <= count; ++i)
			lines.push(String(i))
		return lines.join("\n")
	}

	function closeImmediate() {
		Workspace.closeDialog()
	}

	function executeAction(action) {
		if (!action)
			return
		dialog.forceActiveFocus()
		if (action.type === "close") {
			closeImmediate()
		} else if (action.type === "load") {
			creatingNew = false
			confirmingDelete = false
			editor.loadFile(action.file)
		} else if (action.type === "new") {
			confirmingDelete = false
			newProfileLabelField.text = ""
			cloneProfileSwitch.checked = true
			creatingNew = true
			newProfileLabelField.inputItem.forceActiveFocus()
		} else if (action.type === "activate") {
			creatingNew = false
			confirmingDelete = false
			editor.activateProfile(action.file)
		} else if (action.type === "reload") {
			creatingNew = false
			confirmingDelete = false
			editor.reloadFromDisk()
		}
	}

	function guardOrRun(action) {
		if (editor.dirty) {
			creatingNew = false
			confirmingDelete = false
			pendingDirtyAction = action
			return
		}
		pendingDirtyAction = null
		executeAction(action)
	}

	function requestClose() {
		guardOrRun({ type: "close" })
	}

	function triggerSave() {
		if (editor.busy)
			return
		if (editor.mode === "json" && editor.parseError !== "")
			return
		dialog.forceActiveFocus()
		editor.save()
	}

	function categoryBadge(catId) {
		void editor.revision
		if (catId === "providers") {
			const n = editor.keysAt(["provider"]).length
			return n > 0 ? String(n) : ""
		}
		if (catId === "agents") {
			const n = editor.keysAt(["agent"]).length
			return n > 0 ? String(n) : ""
		}
		if (catId === "mcp") {
			const n = editor.keysAt(["mcp"]).length
			return n > 0 ? String(n) : ""
		}
		if (catId === "commands") {
			const n = editor.keysAt(["command"]).length
			return n > 0 ? String(n) : ""
		}
		return ""
	}

	function selectCategory(catId) {
		if (selectedCategory === catId)
			return
		selectedCategory = catId
		sectionScroller.contentY = 0
	}

	anchors.fill: parent
	focus: true

	Component.onCompleted: {
		entered = true
		forceActiveFocus()
		editor.open(Workspace.effectiveCwd)
		profileSelect.value = editor.selectedFile
		rawMutating = true
		rawEditor.text = editor.rawText
		rawMutating = false
	}

	Keys.onPressed: event => {
		if ((event.modifiers & (Qt.ControlModifier | Qt.MetaModifier)) && event.key === Qt.Key_S) {
			dialog.triggerSave()
			event.accepted = true
			return
		}
		if (event.key === Qt.Key_Escape) {
			if (dialog.pendingDirtyAction !== null) {
				dialog.pendingDirtyAction = null
				event.accepted = true
				return
			}
			if (dialog.creatingNew) {
				dialog.creatingNew = false
				event.accepted = true
				return
			}
			if (dialog.confirmingDelete) {
				dialog.confirmingDelete = false
				event.accepted = true
				return
			}
			dialog.requestClose()
			event.accepted = true
		}
	}

	TapHandler {
		onTapped: dialog.requestClose()
	}

	Connections {
		target: dialog.editor
		function onFilesChanged() {
			profileSelect.value = dialog.editor.selectedFile
		}
		function onRawTextChanged() {
			if (!dialog.rawMutating && rawEditor.text !== dialog.editor.rawText) {
				dialog.rawMutating = true
				rawEditor.text = dialog.editor.rawText
				dialog.rawMutating = false
			}
		}
		function onSaved() {
			if (dialog.continueAfterSave !== null) {
				const next = dialog.continueAfterSave
				dialog.continueAfterSave = null
				dialog.executeAction(next)
			}
		}
	}

	Component {
		id: generalComp
		ConfigGeneralSection { editor: dialog.editor }
	}
	Component {
		id: providersComp
		ConfigProvidersSection { editor: dialog.editor }
	}
	Component {
		id: agentsComp
		ConfigAgentsSection { editor: dialog.editor }
	}
	Component {
		id: permissionsComp
		ConfigPermissionsSection { editor: dialog.editor }
	}
	Component {
		id: mcpComp
		ConfigMcpSection { editor: dialog.editor }
	}
	Component {
		id: commandsComp
		ConfigCommandsSection { editor: dialog.editor }
	}

	component StatusBanner: Rectangle {
		id: banner
		property string tone: "info" // "info" | "warn" | "error"
		property string message: ""

		width: parent ? parent.width : 600
		implicitHeight: bannerLabel.implicitHeight + 16
		radius: Theme.radiusSmall + 2
		color: tone === "error" ? Theme.errorSurface : (tone === "warn" ? Theme.warningSurface : Theme.codeBackground)
		border.color: tone === "error" ? Theme.errorBorder : (tone === "warn" ? Theme.warningBorder : Theme.border)

		Icon {
			id: bannerIcon
			x: 12
			y: 9
			name: banner.tone === "error" ? "error" : (banner.tone === "warn" ? "warning" : "info")
			size: 16
			color: banner.tone === "error" ? Theme.dangerText : (banner.tone === "warn" ? Theme.warningText : Theme.infoText)
		}

		Text {
			id: bannerLabel
			textFormat: Text.PlainText
			anchors {
				left: bannerIcon.right
				leftMargin: 8
				right: parent.right
				rightMargin: 12
				verticalCenter: parent.verticalCenter
			}
			text: banner.message
			color: banner.tone === "error" ? Theme.dangerText : (banner.tone === "warn" ? Theme.warningText : Theme.text)
			font.pixelSize: Theme.smallFontSize
			wrapMode: Text.Wrap
		}
	}

	Rectangle {
		id: panel
		objectName: "configDialogPanel"

		readonly property bool shown: dialog.entered && dialog.open
		property real dropOffset: shown ? 0 : Motion.dialogDrop

		width: Math.min(Theme.configDialogWidth, dialog.width - 2 * Theme.overlayEdgeMargin)
		height: Math.min(Math.round(dialog.height * 0.84), Math.max(420, dialog.height - 2 * Theme.overlayEdgeMargin))
		x: Math.round((dialog.width - width) / 2)
		y: Math.max(Theme.overlayEdgeMargin, Math.round((dialog.height - height) / 2 - dropOffset))
		radius: Theme.radiusLarge
		color: Theme.surface
		border.color: Theme.border
		clip: true
		opacity: shown ? 1 : 0
		scale: shown ? 1 : Motion.popScale

		Behavior on dropOffset { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on opacity { NumberAnimation { duration: panel.shown ? Motion.standard : Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		TapHandler { gesturePolicy: TapHandler.ReleaseWithinBounds }

		// ---- Header + Toolbar + Banners ----
		Column {
			id: topChrome
			width: parent.width
			spacing: 0

			// Title row
			Item {
				width: parent.width
				height: 52

				Row {
					anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: closeButton.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
					spacing: 10

					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						text: qsTr("Configuration")
						color: Theme.text
						font { pixelSize: Theme.fontSize + 1; bold: true }
					}

					Rectangle {
						anchors.verticalCenter: parent.verticalCenter
						width: statusBadgeText.implicitWidth + 12
						height: 20
						radius: 10
						color: dialog.editor.selectedFileActive ? Theme.diffAddedBackground : Theme.warningSurface
						border.color: dialog.editor.selectedFileActive ? Theme.success : Theme.warningBorder

						Text {
							id: statusBadgeText
							textFormat: Text.PlainText
							anchors.centerIn: parent
							text: dialog.editor.selectedFileActive ? qsTr("Active") : qsTr("Backup")
							color: dialog.editor.selectedFileActive ? Theme.successText : Theme.warningText
							font.pixelSize: Theme.microFontSize
						}
					}

					Rectangle {
						visible: dialog.editor.dirty
						anchors.verticalCenter: parent.verticalCenter
						width: dirtyBadgeText.implicitWidth + 12
						height: 20
						radius: 10
						color: Theme.userBubble
						border.color: Theme.userBubbleBorder

						Text {
							id: dirtyBadgeText
							textFormat: Text.PlainText
							anchors.centerIn: parent
							text: qsTr("Unsaved")
							color: Theme.accentText
							font.pixelSize: Theme.microFontSize
						}
					}

					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						width: Math.max(0, parent.width - x)
						text: dialog.editor.configDir !== ""
							? (dialog.editor.configDir + "/" + dialog.editor.selectedFile)
							: dialog.editor.selectedFile
						color: Theme.textMuted
						font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
						elide: Text.ElideMiddle
					}
				}

				IconButton {
					id: closeButton
					objectName: "configCloseButton"
					anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
					buttonSize: 28
					iconName: "close"
					Accessible.name: qsTr("Close")
					onClicked: dialog.requestClose()
				}
			}

			// Profile & Mode toolbar
			Item {
				width: parent.width
				height: 48

				SelectField {
					id: profileSelect
					objectName: "configProfileSelect"
					anchors {
						left: parent.left
						leftMargin: Theme.dialogPadding
						right: toolbarRight.left
						rightMargin: 10
						verticalCenter: parent.verticalCenter
					}
					options: dialog.profileOptions
					placeholderText: "ref.json"
					enabled: !dialog.editor.busy
					onActivated: targetFile => {
						if (targetFile === "" || targetFile === dialog.editor.selectedFile)
							return
						if (dialog.editor.dirty) {
							profileSelect.value = dialog.editor.selectedFile
							dialog.guardOrRun({ type: "load", file: targetFile })
							return
						}
						dialog.executeAction({ type: "load", file: targetFile })
					}
				}

				Row {
					id: toolbarRight
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					spacing: 6

					DialogButton {
						objectName: "configActivateProfileButton"
						visible: !dialog.editor.selectedFileActive
						anchors.verticalCenter: parent.verticalCenter
						small: true
						kind: DialogButton.Primary
						enabled: !dialog.editor.busy
						text: qsTr("Set Active")
						onClicked: dialog.guardOrRun({ type: "activate", file: dialog.editor.selectedFile })
					}

					DialogButton {
						objectName: "configNewProfileButton"
						anchors.verticalCenter: parent.verticalCenter
						small: true
						enabled: !dialog.editor.busy
						text: qsTr("+ New")
						onClicked: {
							if (dialog.creatingNew) {
								dialog.creatingNew = false
								return
							}
							dialog.guardOrRun({ type: "new" })
						}
					}

					IconButton {
						objectName: "configDeleteProfileButton"
						anchors.verticalCenter: parent.verticalCenter
						buttonSize: 32
						iconName: "trash"
						iconSize: 16
						enabled: dialog.editor.selectedFileExists && !dialog.editor.busy
						tooltipText: qsTr("Delete profile")
						Accessible.name: qsTr("Delete profile")
						onClicked: {
							dialog.creatingNew = false
							dialog.pendingDirtyAction = null
							dialog.confirmingDelete = !dialog.confirmingDelete
						}
					}

					IconButton {
						objectName: "configReloadButton"
						anchors.verticalCenter: parent.verticalCenter
						buttonSize: 32
						iconName: "retry"
						iconSize: 16
						enabled: !dialog.editor.busy
						tooltipText: qsTr("Reload configuration from disk")
						Accessible.name: qsTr("Reload configuration from disk")
						onClicked: dialog.guardOrRun({ type: "reload" })
					}

					Segmented {
						id: modeTabs
						objectName: "configModeTabs"
						anchors.verticalCenter: parent.verticalCenter
						width: 156
						height: 30
						model: [
							{ text: qsTr("Visual"), value: "visual" },
							{ text: qsTr("JSON"), value: "json" }
						]
						value: dialog.editor.mode
						onActivated: targetMode => {
							dialog.forceActiveFocus()
							dialog.editor.setMode(targetMode)
						}
					}
				}
			}

			// Inline + New profile bar
			Rectangle {
				visible: dialog.creatingNew
				width: parent.width
				height: visible ? 54 : 0
				color: Theme.codeBackground

				Rectangle { width: parent.width; height: 1; color: Theme.border }

				Item {
					anchors { fill: parent; leftMargin: Theme.dialogPadding; rightMargin: Theme.dialogPadding }

					DialogField {
						id: newProfileLabelField
						objectName: "newProfileLabelField"
						anchors { left: parent.left; right: newBarRight.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
						placeholderText: qsTr("New profile label (e.g. Work, Local vLLM)")
						onAccepted: {
							dialog.editor.createProfile(newProfileLabelField.text, cloneProfileSwitch.checked)
							dialog.creatingNew = false
						}
					}

					Row {
						id: newBarRight
						anchors { right: parent.right; verticalCenter: parent.verticalCenter }
						spacing: 8

						Text {
							textFormat: Text.PlainText
							anchors.verticalCenter: parent.verticalCenter
							text: qsTr("Clone current")
							color: Theme.textMuted
							font.pixelSize: Theme.smallFontSize
						}

						Switch {
							id: cloneProfileSwitch
							objectName: "newProfileCloneSwitch"
							anchors.verticalCenter: parent.verticalCenter
							checked: true
							accessibleName: qsTr("Clone current")
							onToggled: val => checked = val
						}

						DialogButton {
							objectName: "confirmCreateProfileButton"
							anchors.verticalCenter: parent.verticalCenter
							small: true
							kind: DialogButton.Primary
							text: qsTr("Create")
							onClicked: {
								dialog.editor.createProfile(newProfileLabelField.text, cloneProfileSwitch.checked)
								dialog.creatingNew = false
							}
						}

						DialogButton {
							anchors.verticalCenter: parent.verticalCenter
							small: true
							text: qsTr("Cancel")
							onClicked: dialog.creatingNew = false
						}
					}
				}
			}

			// Inline Delete confirmation bar
			Rectangle {
				visible: dialog.confirmingDelete
				width: parent.width
				height: visible ? 50 : 0
				color: Theme.errorSurface

				Rectangle { width: parent.width; height: 1; color: Theme.errorBorder }

				Item {
					anchors { fill: parent; leftMargin: Theme.dialogPadding; rightMargin: Theme.dialogPadding }

					Text {
						textFormat: Text.PlainText
						anchors { left: parent.left; right: delBarBtns.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
						text: dialog.editor.selectedFileActive
							? qsTr("Delete %1? The newest backup (if any) will be promoted to active.").arg(dialog.editor.selectedFile)
							: qsTr("Permanently delete backup profile %1?").arg(dialog.editor.selectedFile)
						color: Theme.dangerText
						font.pixelSize: Theme.smallFontSize
						elide: Text.ElideRight
					}

					Row {
						id: delBarBtns
						anchors { right: parent.right; verticalCenter: parent.verticalCenter }
						spacing: 6

						DialogButton {
							objectName: "confirmDeleteProfileButton"
							small: true
							kind: DialogButton.Danger
							text: qsTr("Delete")
							onClicked: {
								dialog.confirmingDelete = false
								dialog.editor.deleteProfile(dialog.editor.selectedFile)
							}
						}

						DialogButton {
							small: true
							text: qsTr("Cancel")
							onClicked: dialog.confirmingDelete = false
						}
					}
				}
			}

			// Override & diagnostic banners
			Column {
				id: bannersColumn
				x: Theme.dialogPadding
				width: parent.width - 2 * Theme.dialogPadding
				topPadding: visibleChildren.length > 0 ? 4 : 0
				bottomPadding: visibleChildren.length > 0 ? 8 : 0
				spacing: 6

				StatusBanner {
					visible: dialog.editor.hasJsoncOverride
					tone: "warn"
					message: qsTr("ref.jsonc exists in this config directory and takes precedence over ref.json at runtime.")
				}

				StatusBanner {
					visible: dialog.editor.envConfigOverride !== ""
					tone: "warn"
					message: qsTr("Explicit environment config file active: %1").arg(dialog.editor.envConfigOverride)
				}

				StatusBanner {
					visible: dialog.editor.projectOverrides.length > 0
					tone: "info"
					message: qsTr("Project config files merged over global settings: %1").arg(dialog.editor.projectOverrides.join(", "))
				}

				StatusBanner {
					visible: !dialog.editor.selectedFileExists && !dialog.editor.loading
					tone: "info"
					message: qsTr("%1 does not exist on disk yet — click Save to create it.").arg(dialog.editor.selectedFile)
				}

				StatusBanner {
					objectName: "configParseErrorBanner"
					visible: dialog.editor.parseError !== ""
					tone: "error"
					message: qsTr("JSONC syntax error: %1").arg(dialog.editor.parseError)
				}

				StatusBanner {
					visible: dialog.editor.errorText !== "" && dialog.editor.errorText !== dialog.editor.parseError
					tone: "error"
					message: dialog.editor.errorText
				}

				StatusBanner {
					visible: dialog.editor.warnings.length > 0
					tone: "warn"
					message: dialog.editor.warnings.join("\n")
				}
			}

			Rectangle { width: parent.width; height: 1; color: Theme.border }
		}

		// ---- Main Body (Visual Two-Pane OR Raw JSONC) ----
		Item {
			id: mainWorkspace
			anchors {
				top: topChrome.bottom
				left: parent.left
				right: parent.right
				bottom: bottomChrome.top
			}

			// Visual mode
			Item {
				visible: dialog.editor.mode === "visual"
				anchors.fill: parent

				// Left navigation rail
				Rectangle {
					id: leftRail
					width: 204
					height: parent.height
					color: Theme.surface

					Column {
						x: 10
						y: 12
						width: parent.width - 20
						spacing: 4

						Repeater {
							model: dialog.categories

							delegate: Rectangle {
								id: navRow
								required property var modelData
								objectName: "configCategory_" + modelData.id
								readonly property bool active: dialog.selectedCategory === modelData.id
								readonly property string countText: dialog.categoryBadge(modelData.id)

								width: parent.width
								height: 38
								radius: Theme.radiusSmall + 2
								color: active ? Theme.userBubble : (navHover.hovered ? Theme.surfaceHover : "transparent")
								border.width: active ? 1 : 0
								border.color: Theme.userBubbleBorder

								Icon {
									id: navIcon
									x: 10
									anchors.verticalCenter: parent.verticalCenter
									name: navRow.modelData.icon
									size: 16
									color: navRow.active ? Theme.accentText : Theme.textMuted
								}

								Text {
									textFormat: Text.PlainText
									anchors {
										left: navIcon.right
										leftMargin: 10
										right: navBadge.visible ? navBadge.left : parent.right
										rightMargin: 8
										verticalCenter: parent.verticalCenter
									}
									text: navRow.modelData.title
									color: navRow.active ? Theme.text : Theme.textMuted
									font { pixelSize: Theme.secondaryFontSize; bold: navRow.active }
									elide: Text.ElideRight
								}

								Rectangle {
									id: navBadge
									visible: navRow.countText !== ""
									anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
									width: Math.max(20, navBadgeLabel.implicitWidth + 10)
									height: 18
									radius: 9
									color: Theme.codeBackground
									border.color: Theme.border

									Text {
										id: navBadgeLabel
										textFormat: Text.PlainText
										anchors.centerIn: parent
										text: navRow.countText
										color: Theme.textMuted
										font.pixelSize: Theme.microFontSize
									}
								}

								HoverHandler { id: navHover; cursorShape: Qt.PointingHandCursor }
								TapHandler { onTapped: dialog.selectCategory(navRow.modelData.id) }
							}
						}
					}
				}

				Rectangle {
					id: railDivider
					x: leftRail.width
					width: 1
					height: parent.height
					color: Theme.border
				}

				Flickable {
					id: sectionScroller
					anchors {
						left: railDivider.right
						right: parent.right
						top: parent.top
						bottom: parent.bottom
					}
					contentWidth: width
					contentHeight: sectionLoader.implicitHeight + 36
					boundsBehavior: Flickable.StopAtBounds
					clip: true

					ScrollBar.vertical: ThemedScrollBar {}

					Loader {
						id: sectionLoader
						x: 22
						y: 18
						width: sectionScroller.width - 44
						sourceComponent: {
							switch (dialog.selectedCategory) {
							case "providers": return providersComp
							case "agents": return agentsComp
							case "permissions": return permissionsComp
							case "mcp": return mcpComp
							case "commands": return commandsComp
							default: return generalComp
							}
						}
					}
				}
			}

			// Raw JSONC mode
			Rectangle {
				visible: dialog.editor.mode === "json"
				anchors.fill: parent
				color: Theme.codeBackground

				Flickable {
					id: jsonScroller
					anchors.fill: parent
					contentWidth: Math.max(width, gutterBox.width + rawEditor.contentWidth + 32)
					contentHeight: Math.max(height, rawEditor.contentHeight + 28)
					boundsBehavior: Flickable.StopAtBounds
					clip: true

					ScrollBar.vertical: ThemedScrollBar {}
					ScrollBar.horizontal: ThemedScrollBar { orientation: Qt.Horizontal }

					Rectangle {
						id: gutterBox
						width: Math.max(44, lineNumText.implicitWidth + 20)
						height: Math.max(jsonScroller.height, jsonScroller.contentHeight)
						color: Theme.surface

						Text {
							id: lineNumText
							textFormat: Text.PlainText
							x: 8
							y: 14
							width: parent.width - 18
							horizontalAlignment: Text.AlignRight
							text: dialog.lineNumbersText
							color: Theme.textMuted
							font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						}

						Rectangle {
							anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
							width: 1
							color: Theme.border
						}
					}

					TextEdit {
						id: rawEditor
						objectName: "configRawJsonEditor"
						textFormat: TextEdit.PlainText
						x: gutterBox.width + 14
						y: 14
						width: Math.max(jsonScroller.width - gutterBox.width - 28, contentWidth + 16)
						height: Math.max(jsonScroller.height - 28, contentHeight)
						selectByMouse: true
						wrapMode: TextEdit.NoWrap
						tabStopDistance: 16
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
						Accessible.name: qsTr("Raw JSONC configuration")
						onTextChanged: {
							if (!dialog.rawMutating && dialog.editor.mode === "json") {
								dialog.rawMutating = true
								dialog.editor.setRawText(text)
								dialog.rawMutating = false
							}
						}
						Keys.onTabPressed: event => {
							rawEditor.insert(rawEditor.cursorPosition, "  ")
							event.accepted = true
						}
					}
				}
			}
		}

		// ---- Bottom Dirty Confirmation + Footer ----
		Column {
			id: bottomChrome
			anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
			spacing: 0

			// Unsaved changes guard bar
			Rectangle {
				objectName: "configDirtyConfirmBar"
				visible: dialog.pendingDirtyAction !== null
				width: parent.width
				height: visible ? 50 : 0
				color: Theme.warningSurface

				Rectangle { width: parent.width; height: 1; color: Theme.warningBorder }

				Item {
					anchors { fill: parent; leftMargin: Theme.dialogPadding; rightMargin: Theme.dialogPadding }

					Text {
						textFormat: Text.PlainText
						anchors { left: parent.left; right: dirtyActionBtns.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
						text: qsTr("Unsaved changes in %1.").arg(dialog.editor.selectedFile)
						color: Theme.warningText
						font { pixelSize: Theme.smallFontSize; bold: true }
						elide: Text.ElideRight
					}

					Row {
						id: dirtyActionBtns
						anchors { right: parent.right; verticalCenter: parent.verticalCenter }
						spacing: 6

						DialogButton {
							small: true
							text: qsTr("Cancel")
							onClicked: dialog.pendingDirtyAction = null
						}

						DialogButton {
							objectName: "configDirtyDiscardButton"
							small: true
							kind: DialogButton.Danger
							text: qsTr("Discard & Continue")
							onClicked: {
								const act = dialog.pendingDirtyAction
								dialog.pendingDirtyAction = null
								dialog.forceActiveFocus()
								dialog.editor.discard()
								dialog.executeAction(act)
							}
						}

						DialogButton {
							objectName: "configDirtySaveButton"
							small: true
							kind: DialogButton.Primary
							enabled: !dialog.editor.busy && (dialog.editor.mode !== "json" || dialog.editor.parseError === "")
							text: qsTr("Save & Continue")
							onClicked: {
								dialog.continueAfterSave = dialog.pendingDirtyAction
								dialog.pendingDirtyAction = null
								dialog.triggerSave()
							}
						}
					}
				}
			}

			Rectangle { width: parent.width; height: 1; color: Theme.border }

			// Footer bar
			Item {
				width: parent.width
				height: 58

				Row {
					anchors { left: parent.left; leftMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					spacing: 10

					KeyChips {
						anchors.verticalCenter: parent.verticalCenter
						keys: ["Ctrl+S"]
					}

					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						text: {
							if (dialog.editor.saving)
								return qsTr("Saving…")
							if (dialog.editor.loading)
								return qsTr("Loading…")
							if (dialog.editor.dirty)
								return qsTr("Modified — save to apply across live sessions")
							return qsTr("Changes apply live on save")
						}
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}
				}

				Row {
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					spacing: 10

					DialogButton {
						objectName: "configDiscardButton"
						text: qsTr("Discard")
						enabled: dialog.editor.dirty && !dialog.editor.busy
						onClicked: {
							dialog.pendingDirtyAction = null
							dialog.forceActiveFocus()
							dialog.editor.discard()
						}
					}

					DialogButton {
						objectName: "configSaveButton"
						kind: DialogButton.Primary
						text: dialog.editor.saving ? qsTr("Saving…") : qsTr("Save")
						enabled: !dialog.editor.busy
							&& (dialog.editor.dirty || !dialog.editor.selectedFileExists)
							&& (dialog.editor.mode !== "json" || dialog.editor.parseError === "")
						onClicked: dialog.triggerSave()
					}
				}
			}
		}
	}
}
