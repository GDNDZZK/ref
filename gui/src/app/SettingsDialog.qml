import QtQuick
import RefGui

// Theme, language and notifications.
ModalDialog {
	id: dialog

	readonly property int rowHeight: 48

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

		Item {
			objectName: "uiScaleRow"
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
					implicitWidth: 28
					implicitHeight: 28
					enabled: Workspace.settings.uiScale > 0.701
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
				Text {
					objectName: "uiScaleLabel"
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					width: 48
					horizontalAlignment: Text.AlignHCenter
					text: Math.round(Workspace.settings.uiScale * 100) + "%"
					color: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.secondaryFontSize }
				}
				CircleButton {
					objectName: "uiScaleInButton"
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: 28
					implicitHeight: 28
					enabled: Workspace.settings.uiScale < 1.499
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
