import QtQuick
import RefGui

// Theme, language and notifications.
ModalDialog {
	id: dialog

	readonly property int rowHeight: 48
	readonly property int zoomControlHeight: 28
	readonly property int zoomInputWidth: 68
	readonly property int messageWindowInputWidth: 76
	readonly property int panelSizeInputWidth: 64
	readonly property int zoomInputLeftMargin: 6
	readonly property int zoomInputSuffixGap: 2
	readonly property int zoomInputRightMargin: 8

	title: qsTr("Settings")
	dialogWidth: Theme.settingsWidth

	Column {
		width: parent.width
		bottomPadding: 18

		SectionLabel { text: qsTr("Appearance") }

		Item {
			width: parent.width
			height: dialog.rowHeight

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: themeTabs.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: qsTr("Theme")
				color: Theme.text
				font.pixelSize: Theme.fontSize
				elide: Text.ElideRight
			}
			Segmented {
				id: themeTabs
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				width: 230
				model: [
					{text: qsTr("System"), value: ThemeName.system},
					{text: qsTr("Light"), value: ThemeName.light},
					{text: qsTr("Dark"), value: ThemeName.dark}
				]
				value: Workspace.settings.theme
				onActivated: value => Workspace.requestTheme(value)
			}
		}

		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		Item {
			width: parent.width
			height: dialog.rowHeight

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: langTabs.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: qsTr("Language")
				color: Theme.text
				font.pixelSize: Theme.fontSize
				elide: Text.ElideRight
			}
			Segmented {
				id: langTabs
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				width: 280
				model: [
					{text: qsTr("System"), value: "system"},
					{text: "English", value: "en"},
					{text: "简体中文", value: "zh_CN"}
				]
				value: Workspace.settings.language
				onActivated: value => Workspace.settings.language = value
			}
		}

		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		Column {
			objectName: "uiScaleRow"
			width: parent.width
			bottomPadding: restartHint.visible ? 10 : 0

			Item {
				width: parent.width
				height: dialog.rowHeight

				Text {
					textFormat: Text.PlainText
					anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: zoomControls.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
					text: qsTr("Interface zoom")
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}
				Row {
					id: zoomControls
					anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
					spacing: 8

					TextButton {
						objectName: "uiScaleResetButton"
						anchors.verticalCenter: parent.verticalCenter
						visible: Math.abs(Workspace.settings.uiScale - 1.0) > 0.001
						text: qsTr("Reset")
						onClicked: Commands.run(CommandId.uiZoomReset)
					}
					CircleButton {
						objectName: "uiScaleOutButton"
						anchors.verticalCenter: parent.verticalCenter
						implicitWidth: dialog.zoomControlHeight
						implicitHeight: dialog.zoomControlHeight
						enabled: Workspace.settings.uiScale > Workspace.settings.minUiScale + 0.001
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Interface zoom") + " (−)"
						onClicked: Commands.run(CommandId.uiZoomOut)

						Text {
							textFormat: Text.PlainText
							anchors.centerIn: parent
							text: "−"
							color: Theme.text
							font { pixelSize: Theme.fontSize; bold: true }
						}
					}
					Rectangle {
						id: uiScaleBox
						anchors.verticalCenter: parent.verticalCenter
						width: dialog.zoomInputWidth
						height: dialog.zoomControlHeight
						radius: Theme.radiusSmall
						color: Theme.background
						border.color: uiScaleInput.activeFocus ? Theme.accentText : Theme.border

						Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

						function formattedPercent() {
							return String(Math.round(Workspace.settings.uiScale * 100))
						}

						function commit() {
							let raw = uiScaleInput.text.trim()
							if (raw.endsWith("%"))
								raw = raw.slice(0, -1).trim()
							const parsed = raw !== "" ? Number(raw) : NaN
							if (!Number.isFinite(parsed) || parsed <= 0) {
								uiScaleInput.text = formattedPercent()
								return
							}
							const minPct = Math.round(Workspace.settings.minUiScale * 100)
							const maxPct = Math.round(Workspace.settings.maxUiScale * 100)
							const clamped = Math.min(maxPct, Math.max(minPct, Math.round(parsed)))
							Workspace.settings.uiScale = clamped / 100.0
							uiScaleInput.text = formattedPercent()
						}

						Connections {
							target: Workspace.settings
							function onUiScaleChanged() {
								uiScaleInput.text = uiScaleBox.formattedPercent()
							}
						}

						TextInput {
							id: uiScaleInput
							objectName: "uiScaleInput"
							anchors { left: parent.left; leftMargin: dialog.zoomInputLeftMargin; right: percentSuffix.left; rightMargin: dialog.zoomInputSuffixGap; top: parent.top; bottom: parent.bottom }
							horizontalAlignment: TextInput.AlignRight
							verticalAlignment: TextInput.AlignVCenter
							color: Theme.text
							selectionColor: Theme.selection
							selectedTextColor: Theme.text
							font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
							clip: true
							selectByMouse: true
							Accessible.name: qsTr("Interface zoom")
							Component.onCompleted: text = uiScaleBox.formattedPercent()
							onAccepted: {
								uiScaleBox.commit()
								dialog.forceActiveFocus()
							}
							onEditingFinished: if (text !== uiScaleBox.formattedPercent()) uiScaleBox.commit()
							Keys.onEscapePressed: event => {
								text = uiScaleBox.formattedPercent()
								dialog.forceActiveFocus()
								event.accepted = true
							}
							Keys.onUpPressed: event => {
								Commands.run(CommandId.uiZoomIn)
								text = uiScaleBox.formattedPercent()
								event.accepted = true
							}
							Keys.onDownPressed: event => {
								Commands.run(CommandId.uiZoomOut)
								text = uiScaleBox.formattedPercent()
								event.accepted = true
							}
						}

						Text {
							id: percentSuffix
							textFormat: Text.PlainText
							anchors { right: parent.right; rightMargin: dialog.zoomInputRightMargin; verticalCenter: parent.verticalCenter }
							text: "%"
							color: Theme.textMuted
							font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
							TapHandler { onTapped: uiScaleInput.forceActiveFocus() }
						}
					}
					CircleButton {
						objectName: "uiScaleInButton"
						anchors.verticalCenter: parent.verticalCenter
						implicitWidth: dialog.zoomControlHeight
						implicitHeight: dialog.zoomControlHeight
						enabled: Workspace.settings.uiScale < Workspace.settings.maxUiScale - 0.001
						opacity: enabled ? 1 : Theme.disabledOpacity
						Accessible.name: qsTr("Interface zoom") + " (+)"
						onClicked: Commands.run(CommandId.uiZoomIn)

						Text {
							textFormat: Text.PlainText
							anchors.centerIn: parent
							text: "+"
							color: Theme.text
							font { pixelSize: Theme.fontSize; bold: true }
						}
					}
				}
			}

			Text {
				id: restartHint
				objectName: "uiScaleRestartHint"
				textFormat: Text.PlainText
				visible: Math.abs(Workspace.settings.uiScale - Workspace.startupUiScale) > 0.001
				x: Theme.dialogPadding
				width: parent.width - 2 * Theme.dialogPadding
				text: qsTr("Restart ref-gui for native sharpness")
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				wrapMode: Text.Wrap
			}
		}
		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		Item {
			objectName: "messageWindowRow"
			width: parent.width
			height: 54

			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: messageWindowControls.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Message window")
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("0 = unlimited (load all)")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}

			Row {
				id: messageWindowControls
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				spacing: 8

				TextButton {
					objectName: "messageWindowResetButton"
					anchors.verticalCenter: parent.verticalCenter
					visible: Workspace.settings.messageWindowSize !== Workspace.settings.defaultMessageWindowSize
					text: qsTr("Reset")
					onClicked: Workspace.settings.messageWindowSize = Workspace.settings.defaultMessageWindowSize
				}
				CircleButton {
					objectName: "messageWindowOutButton"
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: dialog.zoomControlHeight
					implicitHeight: dialog.zoomControlHeight
					enabled: Workspace.settings.messageWindowSize > 0
					opacity: enabled ? 1 : Theme.disabledOpacity
					Accessible.name: qsTr("Message window") + " (−)"
					onClicked: Workspace.settings.messageWindowSize = Math.max(0, Workspace.settings.messageWindowSize - Workspace.settings.messageWindowStep)

					Text {
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: "−"
						color: Theme.text
						font { pixelSize: Theme.fontSize; bold: true }
					}
				}
				Rectangle {
					id: messageWindowBox
					anchors.verticalCenter: parent.verticalCenter
					width: dialog.messageWindowInputWidth
					height: dialog.zoomControlHeight
					radius: Theme.radiusSmall
					color: Theme.background
					border.color: messageWindowInput.activeFocus ? Theme.accentText : Theme.border

					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					function formattedSize() {
						return String(Workspace.settings.messageWindowSize)
					}

					function commit() {
						const raw = messageWindowInput.text.trim()
						if (raw === "" || !/^\d+$/.test(raw)) {
							messageWindowInput.text = formattedSize()
							return
						}
						const parsed = Number(raw)
						if (!Number.isSafeInteger(parsed) || parsed < 0 || parsed > 2147483647) {
							messageWindowInput.text = formattedSize()
							return
						}
						Workspace.settings.messageWindowSize = parsed
						messageWindowInput.text = formattedSize()
					}

					Connections {
						target: Workspace.settings
						function onMessageWindowSizeChanged() {
							messageWindowInput.text = messageWindowBox.formattedSize()
						}
					}

					TapHandler { onTapped: messageWindowInput.forceActiveFocus() }
					TextInput {
						id: messageWindowInput
						objectName: "messageWindowInput"
						anchors { left: parent.left; leftMargin: dialog.zoomInputRightMargin; right: parent.right; rightMargin: dialog.zoomInputRightMargin; top: parent.top; bottom: parent.bottom }
						horizontalAlignment: TextInput.AlignRight
						verticalAlignment: TextInput.AlignVCenter
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
						clip: true
						selectByMouse: true
						Accessible.name: qsTr("Message window")
						Component.onCompleted: text = messageWindowBox.formattedSize()
						onAccepted: {
							messageWindowBox.commit()
							dialog.forceActiveFocus()
						}
						onEditingFinished: if (text !== messageWindowBox.formattedSize()) messageWindowBox.commit()
						Keys.onEscapePressed: event => {
							text = messageWindowBox.formattedSize()
							dialog.forceActiveFocus()
							event.accepted = true
						}
						Keys.onUpPressed: event => {
							Workspace.settings.messageWindowSize = Workspace.settings.messageWindowSize + Workspace.settings.messageWindowStep
							text = messageWindowBox.formattedSize()
							event.accepted = true
						}
						Keys.onDownPressed: event => {
							Workspace.settings.messageWindowSize = Math.max(0, Workspace.settings.messageWindowSize - Workspace.settings.messageWindowStep)
							text = messageWindowBox.formattedSize()
							event.accepted = true
						}
					}
				}
				CircleButton {
					objectName: "messageWindowInButton"
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: dialog.zoomControlHeight
					implicitHeight: dialog.zoomControlHeight
					Accessible.name: qsTr("Message window") + " (+)"
					onClicked: Workspace.settings.messageWindowSize = Workspace.settings.messageWindowSize + Workspace.settings.messageWindowStep

					Text {
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: "+"
						color: Theme.text
						font { pixelSize: Theme.fontSize; bold: true }
					}
				}
			}
		}
		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		Item {
			objectName: "floatingBallSizeRow"
			width: parent.width
			height: 54

			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: floatingBallSizeControls.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Floating ball size")
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Diameter in pixels (%1–%2)").arg(Workspace.settings.minFloatingBallSize).arg(Workspace.settings.maxFloatingBallSize)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}

			Row {
				id: floatingBallSizeControls
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				spacing: 8

				TextButton {
					objectName: "floatingBallSizeResetButton"
					anchors.verticalCenter: parent.verticalCenter
					visible: Workspace.settings.floatingBallSize !== Workspace.settings.defaultFloatingBallSize
					text: qsTr("Reset")
					onClicked: Workspace.settings.floatingBallSize = Workspace.settings.defaultFloatingBallSize
				}
				CircleButton {
					objectName: "floatingBallSizeOutButton"
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: dialog.zoomControlHeight
					implicitHeight: dialog.zoomControlHeight
					enabled: Workspace.settings.floatingBallSize > Workspace.settings.minFloatingBallSize
					opacity: enabled ? 1 : Theme.disabledOpacity
					Accessible.name: qsTr("Floating ball size") + " (−)"
					onClicked: Workspace.settings.floatingBallSize = Math.max(Workspace.settings.minFloatingBallSize, Workspace.settings.floatingBallSize - Workspace.settings.floatingBallSizeStep)

					Text {
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: "−"
						color: Theme.text
						font { pixelSize: Theme.fontSize; bold: true }
					}
				}
				Rectangle {
					id: floatingBallSizeBox
					anchors.verticalCenter: parent.verticalCenter
					width: dialog.zoomInputWidth
					height: dialog.zoomControlHeight
					radius: Theme.radiusSmall
					color: Theme.background
					border.color: floatingBallSizeInput.activeFocus ? Theme.accentText : Theme.border

					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					function formattedSize() {
						return String(Workspace.settings.floatingBallSize)
					}

					function commit() {
						const raw = floatingBallSizeInput.text.trim()
						if (raw === "" || !/^\d+$/.test(raw)) {
							floatingBallSizeInput.text = formattedSize()
							return
						}
						const parsed = Number(raw)
						if (!Number.isSafeInteger(parsed)) {
							floatingBallSizeInput.text = formattedSize()
							return
						}
						Workspace.settings.floatingBallSize = parsed
						floatingBallSizeInput.text = formattedSize()
					}

					Connections {
						target: Workspace.settings
						function onFloatingBallChanged() {
							floatingBallSizeInput.text = floatingBallSizeBox.formattedSize()
						}
					}

					TextInput {
						id: floatingBallSizeInput
						objectName: "floatingBallSizeInput"
						anchors { left: parent.left; leftMargin: dialog.zoomInputLeftMargin; right: ballPxSuffix.left; rightMargin: dialog.zoomInputSuffixGap; top: parent.top; bottom: parent.bottom }
						horizontalAlignment: TextInput.AlignRight
						verticalAlignment: TextInput.AlignVCenter
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
						clip: true
						selectByMouse: true
						Accessible.name: qsTr("Floating ball size")
						Component.onCompleted: text = floatingBallSizeBox.formattedSize()
						onAccepted: {
							floatingBallSizeBox.commit()
							dialog.forceActiveFocus()
						}
						onEditingFinished: if (text !== floatingBallSizeBox.formattedSize()) floatingBallSizeBox.commit()
						Keys.onEscapePressed: event => {
							text = floatingBallSizeBox.formattedSize()
							dialog.forceActiveFocus()
							event.accepted = true
						}
						Keys.onUpPressed: event => {
							Workspace.settings.floatingBallSize = Math.min(Workspace.settings.maxFloatingBallSize, Workspace.settings.floatingBallSize + Workspace.settings.floatingBallSizeStep)
							text = floatingBallSizeBox.formattedSize()
							event.accepted = true
						}
						Keys.onDownPressed: event => {
							Workspace.settings.floatingBallSize = Math.max(Workspace.settings.minFloatingBallSize, Workspace.settings.floatingBallSize - Workspace.settings.floatingBallSizeStep)
							text = floatingBallSizeBox.formattedSize()
							event.accepted = true
						}
					}

					Text {
						id: ballPxSuffix
						textFormat: Text.PlainText
						anchors { right: parent.right; rightMargin: dialog.zoomInputRightMargin; verticalCenter: parent.verticalCenter }
						text: "px"
						color: Theme.textMuted
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
						TapHandler { onTapped: floatingBallSizeInput.forceActiveFocus() }
					}
				}
				CircleButton {
					objectName: "floatingBallSizeInButton"
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: dialog.zoomControlHeight
					implicitHeight: dialog.zoomControlHeight
					enabled: Workspace.settings.floatingBallSize < Workspace.settings.maxFloatingBallSize
					opacity: enabled ? 1 : Theme.disabledOpacity
					Accessible.name: qsTr("Floating ball size") + " (+)"
					onClicked: Workspace.settings.floatingBallSize = Math.min(Workspace.settings.maxFloatingBallSize, Workspace.settings.floatingBallSize + Workspace.settings.floatingBallSizeStep)

					Text {
						textFormat: Text.PlainText
						anchors.centerIn: parent
						text: "+"
						color: Theme.text
						font { pixelSize: Theme.fontSize; bold: true }
					}
				}
			}
		}
		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		Item {
			objectName: "floatingPanelSizeRow"
			width: parent.width
			height: 54

			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: floatingPanelSizeControls.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Mini panel size")
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Width × height in pixels; also resizable by dragging edges")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}

			Row {
				id: floatingPanelSizeControls
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				spacing: 8

				TextButton {
					objectName: "floatingPanelSizeResetButton"
					anchors.verticalCenter: parent.verticalCenter
					visible: Workspace.settings.floatingPanelWidth !== Workspace.settings.defaultFloatingPanelWidth || Workspace.settings.floatingPanelHeight !== Workspace.settings.defaultFloatingPanelHeight
					text: qsTr("Reset")
					onClicked: Workspace.settings.setFloatingPanelSize(Workspace.settings.defaultFloatingPanelWidth, Workspace.settings.defaultFloatingPanelHeight)
				}
				Rectangle {
					id: floatingPanelWidthBox
					anchors.verticalCenter: parent.verticalCenter
					width: dialog.panelSizeInputWidth
					height: dialog.zoomControlHeight
					radius: Theme.radiusSmall
					color: Theme.background
					border.color: floatingPanelWidthInput.activeFocus ? Theme.accentText : Theme.border

					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					function formattedWidth() {
						return String(Workspace.settings.floatingPanelWidth)
					}

					function commit() {
						const raw = floatingPanelWidthInput.text.trim()
						if (raw === "" || !/^\d+$/.test(raw)) {
							floatingPanelWidthInput.text = formattedWidth()
							return
						}
						const parsed = Number(raw)
						if (!Number.isSafeInteger(parsed)) {
							floatingPanelWidthInput.text = formattedWidth()
							return
						}
						Workspace.settings.floatingPanelWidth = parsed
						floatingPanelWidthInput.text = formattedWidth()
					}

					Connections {
						target: Workspace.settings
						function onFloatingBallChanged() {
							floatingPanelWidthInput.text = floatingPanelWidthBox.formattedWidth()
						}
					}

					TapHandler { onTapped: floatingPanelWidthInput.forceActiveFocus() }
					TextInput {
						id: floatingPanelWidthInput
						objectName: "floatingPanelWidthInput"
						anchors { left: parent.left; leftMargin: dialog.zoomInputRightMargin; right: parent.right; rightMargin: dialog.zoomInputRightMargin; top: parent.top; bottom: parent.bottom }
						horizontalAlignment: TextInput.AlignRight
						verticalAlignment: TextInput.AlignVCenter
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
						clip: true
						selectByMouse: true
						Accessible.name: qsTr("Mini panel size") + " (W)"
						Component.onCompleted: text = floatingPanelWidthBox.formattedWidth()
						onAccepted: {
							floatingPanelWidthBox.commit()
							dialog.forceActiveFocus()
						}
						onEditingFinished: if (text !== floatingPanelWidthBox.formattedWidth()) floatingPanelWidthBox.commit()
						Keys.onEscapePressed: event => {
							text = floatingPanelWidthBox.formattedWidth()
							dialog.forceActiveFocus()
							event.accepted = true
						}
						Keys.onUpPressed: event => {
							Workspace.settings.floatingPanelWidth = Math.min(Workspace.settings.maxFloatingPanelWidth, Workspace.settings.floatingPanelWidth + Workspace.settings.floatingPanelSizeStep)
							text = floatingPanelWidthBox.formattedWidth()
							event.accepted = true
						}
						Keys.onDownPressed: event => {
							Workspace.settings.floatingPanelWidth = Math.max(Workspace.settings.minFloatingPanelWidth, Workspace.settings.floatingPanelWidth - Workspace.settings.floatingPanelSizeStep)
							text = floatingPanelWidthBox.formattedWidth()
							event.accepted = true
						}
					}
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					text: "×"
					color: Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
				}
				Rectangle {
					id: floatingPanelHeightBox
					anchors.verticalCenter: parent.verticalCenter
					width: dialog.panelSizeInputWidth
					height: dialog.zoomControlHeight
					radius: Theme.radiusSmall
					color: Theme.background
					border.color: floatingPanelHeightInput.activeFocus ? Theme.accentText : Theme.border

					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

					function formattedHeight() {
						return String(Workspace.settings.floatingPanelHeight)
					}

					function commit() {
						const raw = floatingPanelHeightInput.text.trim()
						if (raw === "" || !/^\d+$/.test(raw)) {
							floatingPanelHeightInput.text = formattedHeight()
							return
						}
						const parsed = Number(raw)
						if (!Number.isSafeInteger(parsed)) {
							floatingPanelHeightInput.text = formattedHeight()
							return
						}
						Workspace.settings.floatingPanelHeight = parsed
						floatingPanelHeightInput.text = formattedHeight()
					}

					Connections {
						target: Workspace.settings
						function onFloatingBallChanged() {
							floatingPanelHeightInput.text = floatingPanelHeightBox.formattedHeight()
						}
					}

					TapHandler { onTapped: floatingPanelHeightInput.forceActiveFocus() }
					TextInput {
						id: floatingPanelHeightInput
						objectName: "floatingPanelHeightInput"
						anchors { left: parent.left; leftMargin: dialog.zoomInputRightMargin; right: parent.right; rightMargin: dialog.zoomInputRightMargin; top: parent.top; bottom: parent.bottom }
						horizontalAlignment: TextInput.AlignRight
						verticalAlignment: TextInput.AlignVCenter
						color: Theme.text
						selectionColor: Theme.selection
						selectedTextColor: Theme.text
						font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
						clip: true
						selectByMouse: true
						Accessible.name: qsTr("Mini panel size") + " (H)"
						Component.onCompleted: text = floatingPanelHeightBox.formattedHeight()
						onAccepted: {
							floatingPanelHeightBox.commit()
							dialog.forceActiveFocus()
						}
						onEditingFinished: if (text !== floatingPanelHeightBox.formattedHeight()) floatingPanelHeightBox.commit()
						Keys.onEscapePressed: event => {
							text = floatingPanelHeightBox.formattedHeight()
							dialog.forceActiveFocus()
							event.accepted = true
						}
						Keys.onUpPressed: event => {
							Workspace.settings.floatingPanelHeight = Math.min(Workspace.settings.maxFloatingPanelHeight, Workspace.settings.floatingPanelHeight + Workspace.settings.floatingPanelSizeStep)
							text = floatingPanelHeightBox.formattedHeight()
							event.accepted = true
						}
						Keys.onDownPressed: event => {
							Workspace.settings.floatingPanelHeight = Math.max(Workspace.settings.minFloatingPanelHeight, Workspace.settings.floatingPanelHeight - Workspace.settings.floatingPanelSizeStep)
							text = floatingPanelHeightBox.formattedHeight()
							event.accepted = true
						}
					}
				}
			}
		}
		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		// The notification switches are the composer's part of the window.
		NotificationSettingsSection {
			x: Theme.dialogPadding
			width: parent.width - 2 * Theme.dialogPadding
			settings: Workspace.settings
		}

		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		SectionLabel { text: qsTr("Configuration") }

		Item {
			width: parent.width
			height: dialog.rowHeight

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: configButtons.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: "ref.json"
				color: Theme.text
				font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
				elide: Text.ElideRight
			}
			Row {
				id: configButtons
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				spacing: 8

				DialogButton {
					small: true
					text: qsTr("Reload configuration")
					enabled: Workspace.connectionState === "connected"
					onClicked: Commands.run(CommandId.reload)
				}
				DialogButton {
					small: true
					kind: DialogButton.Primary
					text: qsTr("Edit ref.json…")
					enabled: Workspace.connectionState === "connected"
					onClicked: Commands.run(CommandId.config)
				}
			}
		}

		Rectangle { x: Theme.dialogPadding; width: parent.width - 2 * Theme.dialogPadding; height: 1; color: Theme.border }

		SectionLabel { text: qsTr("Backend") }

		Item {
			width: parent.width
			height: 54

			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: backendSwitchButton.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: qsTr("Server connection")
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}

				Text {
					objectName: "settingsBackendSummary"
					textFormat: Text.PlainText
					width: parent.width
					text: (Workspace.backendUrl !== "" ? Workspace.backendUrl : qsTr("Local (auto)"))
						+ (Workspace.maskedBackendToken !== "" ? " · " + Workspace.maskedBackendToken : "")
					color: Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
					elide: Text.ElideMiddle
				}
			}

			DialogButton {
				id: backendSwitchButton
				objectName: "settingsSwitchBackendButton"
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				small: true
				text: qsTr("Switch backend…")
				onClicked: Commands.run(CommandId.backend)
			}
		}

		TextEdit {
			textFormat: TextEdit.PlainText
			x: Theme.dialogPadding
			width: parent.width - 2 * Theme.dialogPadding
			topPadding: 14
			readOnly: true
			selectByMouse: true
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			text: qsTr("ref-gui %1 · backend %2").arg(Qt.application.version).arg(Workspace.backendVersion)
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}
}
