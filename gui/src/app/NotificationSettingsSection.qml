import QtQuick
import QtQuick.Controls.Basic
import RefGui

// The "Notifications" block of the settings panel. `settings` is the window's Settings object.
// Other sessions' messages never notify; the switches choose which events of the session
// the user is not looking at do.
Column {
	id: section

	required property var settings

	readonly property int rowHeight: 40
	readonly property int subIndent: 24
	readonly property int headerIconSize: 20
	readonly property int headerGap: 8
	readonly property int switchWidth: 36
	readonly property int switchHeight: 20
	readonly property int knobInset: 2
	readonly property real disabledOpacity: 0.4

	spacing: 0

	Row {
		height: section.rowHeight
		spacing: section.headerGap

		Icon {
			anchors.verticalCenter: parent.verticalCenter
			name: "bell"
			size: section.headerIconSize
			color: Theme.textMuted
		}

		Text {
			anchors.verticalCenter: parent.verticalCenter
			text: qsTr("Notifications")
			color: Theme.text
			font.pixelSize: Theme.fontSize
			font.bold: true
		}
	}

	SwitchRow {
		text: qsTr("Show system notifications")
		checked: section.settings.notifyEnabled
		onToggled: value => section.settings.notifyEnabled = value
	}

	// Why there is no system notification; the switches still decide when the taskbar flashes instead.
	Text {
		visible: !NotificationCenter.systemNotificationsAvailable
		width: parent.width
		text: qsTr("Not available on this system. The taskbar flashes instead.")
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
		bottomPadding: 4

		HoverHandler { id: reasonHover }
		ToolTip.visible: reasonHover.hovered && NotificationCenter.unavailableReason !== ""
		ToolTip.text: NotificationCenter.unavailableReason
	}

	SwitchRow {
		text: qsTr("A tool needs permission")
		indented: true
		enabled: section.settings.notifyEnabled
		checked: section.settings.notifyPermission
		onToggled: value => section.settings.notifyPermission = value
	}

	SwitchRow {
		text: qsTr("A turn finished")
		indented: true
		enabled: section.settings.notifyEnabled
		checked: section.settings.notifyTurnDone
		onToggled: value => section.settings.notifyTurnDone = value
	}

	SwitchRow {
		text: qsTr("A turn ended with an error")
		indented: true
		enabled: section.settings.notifyEnabled
		checked: section.settings.notifyTurnError
		onToggled: value => section.settings.notifyTurnError = value
	}

	component SwitchRow: Item {
		id: row

		property alias text: label.text
		property bool checked: false
		property bool indented: false
		signal toggled(bool value)

		width: section.width
		height: section.rowHeight
		opacity: enabled ? 1 : section.disabledOpacity

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		Text {
			id: label
			anchors {
				left: parent.left
				leftMargin: row.indented ? section.subIndent : 0
				right: track.left
				rightMargin: section.headerGap
				verticalCenter: parent.verticalCenter
			}
			color: Theme.text
			font.pixelSize: Theme.fontSize
			elide: Text.ElideRight
		}

		Rectangle {
			id: track
			anchors { right: parent.right; verticalCenter: parent.verticalCenter }
			width: section.switchWidth
			height: section.switchHeight
			radius: height / 2
			color: row.checked ? Theme.accent : Theme.border

			Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Rectangle {
				width: parent.height - 2 * section.knobInset
				height: width
				radius: width / 2
				y: section.knobInset
				x: row.checked ? parent.width - width - section.knobInset : section.knobInset
				color: row.checked ? Theme.onAccent : Theme.surface

				Behavior on x { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			}
		}

		HoverHandler { cursorShape: Qt.PointingHandCursor }
		TapHandler { onTapped: row.toggled(!row.checked) }

		Accessible.role: Accessible.CheckBox
		Accessible.name: label.text
		Accessible.checked: row.checked
		Accessible.onToggleAction: row.toggled(!row.checked)
	}
}
