import QtQuick
import RefGui

// Theme, language and notifications.
ModalDialog {
	id: dialog

	readonly property int rowHeight: 48
	readonly property int zoomControlHeight: 28
	readonly property int zoomInputWidth: 68
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
