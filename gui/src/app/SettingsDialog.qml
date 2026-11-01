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

		Text {
			textFormat: Text.PlainText
			width: parent.width
			height: Theme.sidebarSectionHeight
			leftPadding: Theme.dialogPadding
			verticalAlignment: Text.AlignVCenter
			text: qsTr("Appearance")
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}

		Item {
			width: parent.width
			height: dialog.rowHeight

			Text {
				textFormat: Text.PlainText
				x: Theme.dialogPadding
				anchors.verticalCenter: parent.verticalCenter
				text: qsTr("Theme")
				color: Theme.text
				font.pixelSize: Theme.fontSize
			}
			Segmented {
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				width: 230
				model: [
					{text: qsTr("System"), value: "system"},
					{text: qsTr("Light"), value: "light"},
					{text: qsTr("Dark"), value: "dark"}
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
				x: Theme.dialogPadding
				anchors.verticalCenter: parent.verticalCenter
				text: qsTr("Language")
				color: Theme.text
				font.pixelSize: Theme.fontSize
			}
			Segmented {
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

		// The notification switches are the composer's part of the window.
		NotificationSettingsSection {
			x: Theme.dialogPadding
			width: parent.width - 2 * Theme.dialogPadding
			settings: Workspace.settings
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
