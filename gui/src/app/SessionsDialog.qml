import QtQuick
import RefGui

// Live and past sessions in one searchable list; Enter opens one. With the argument
// `mode: "close"` (the close-session command) a click closes a live session instead.
ModalDialog {
	id: dialog

	readonly property bool closing: Workspace.dialogArgs.mode === "close"

	title: closing ? qsTr("Close a session") : qsTr("Switch session")
	onAccepted: list.activateCurrent()

	// Search runs on the picker's own filter, so the sidebar's list is not touched.
	Binding {
		target: Workspace.sessionChoices
		property: "filter"
		value: list.text.trim()
	}

	PickList {
		id: list
		width: parent.width
		placeholderText: qsTr("Search sessions")
		sectionProperty: "isLive"
		sectionLabel: section => section === "true" ? qsTr("Running") : qsTr("History")
		emptyText: qsTr("No matching sessions")
		model: Workspace.sessionChoices
		delegate: PickRow {
			id: row
			required property string sessionId
			required property string displayTitle
			required property string dirName
			required property string activity
			required property bool hasError
			required property bool isLive
			required property bool isCurrent
			required property double updatedAt
			enabled: !dialog.closing || isLive
			onActivated: {
				if (dialog.closing)
					Workspace.closeSession(sessionId)
				else
					Workspace.openSession(sessionId)
				dialog.close()
			}

			SessionDot {
				x: 22
				anchors.verticalCenter: parent.verticalCenter
				activity: row.activity
				hasError: row.hasError
				visible: row.isLive
			}
			Column {
				anchors { left: parent.left; leftMargin: 42; right: tail.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					width: parent.width
					text: row.displayTitle
					color: Theme.text
					font { pixelSize: Theme.fontSize - 2; bold: row.isCurrent }
					elide: Text.ElideRight
				}
				Text {
					width: parent.width
					text: row.isLive ? row.dirName : row.dirName + " · " + Format.relativeTime(row.updatedAt)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}
			Item {
				id: tail
				anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
				width: 22
				height: 22

				Icon {
					anchors.centerIn: parent
					name: "check"
					size: 16
					color: Theme.accent
					visible: row.isCurrent && !closeTap.visible
				}
				CircleButton {
					id: closeTap
					anchors.fill: parent
					visible: row.isLive && row.current && !dialog.closing
					Accessible.name: qsTr("Close session")
					onClicked: Workspace.closeSession(row.sessionId)

					Icon {
						anchors.centerIn: parent
						name: "close"
						size: 16
						color: Theme.textMuted
					}
				}
			}
		}
	}
}
