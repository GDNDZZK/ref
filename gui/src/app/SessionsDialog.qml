pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Live and past sessions in one searchable list; Enter opens one. With the argument
// `mode: "close"` (the close-session command) a click closes a live session instead.
ModalDialog {
	id: dialog

	readonly property bool closing: Workspace.dialogArgs.mode === DialogId.closeMode
	// ListView.section turns the isLive bool into a string.
	readonly property string liveSection: "true"
	property string filterMode: "all"
	property string lockedCreatorId: ""
	property string lockedCreatorName: ""
	readonly property int creatorFilterValue: {
		if (filterMode === "user")
			return SessionList.UserCreated
		if (filterMode === "creator")
			return SessionList.ByCreator
		return SessionList.All
	}
	readonly property SessionPickRow selectedRow: list.currentItem as SessionPickRow
	readonly property string previewCreatorName: {
		if (filterMode === "creator")
			return lockedCreatorName
		if (selectedRow && selectedRow.parentId !== "")
			return selectedRow.creatorName
		return ""
	}
	readonly property var filterTabs: [
		{text: qsTr("All"), value: "all"},
		{text: qsTr("User-created"), value: "user"},
		{text: previewCreatorName !== "" ? qsTr("By %1").arg(previewCreatorName) : qsTr("By creator"), value: "creator"}
	]

	component SessionPickRow: PickRow {
		id: row
		required property string sessionId
		required property string displayTitle
		required property string dirName
		required property string activity
		required property bool hasError
		required property bool isLive
		required property bool isCurrent
		required property double updatedAt
		required property string parentId
		required property string creatorName
		readonly property string detailLine: {
			const base = row.isLive ? row.dirName : row.dirName + " · " + Format.relativeTime(row.updatedAt)
			return row.parentId !== "" ? base + " · \u27f2 " + row.creatorName : base
		}
		enabled: !dialog.closing || isLive
		onActivated: {
			if (dialog.closing) {
				Workspace.requestCloseSession(sessionId)
				if (Workspace.dialog === DialogId.sessions)
					dialog.close()
			} else {
				Workspace.openSession(sessionId)
				dialog.close()
			}
		}

		SessionDot {
			x: Theme.dialogPadding
			anchors.verticalCenter: parent.verticalCenter
			activity: row.activity
			hasError: row.hasError
			visible: row.isLive
		}
		Column {
			anchors { left: parent.left; leftMargin: Theme.dialogPadding + 20; right: tail.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
			spacing: 2

			Text {
				textFormat: Text.PlainText
				width: parent.width
				text: row.displayTitle
				color: Theme.text
				font { pixelSize: Theme.compactFontSize; bold: row.isCurrent }
				elide: Text.ElideRight
			}
			Text {
				textFormat: Text.PlainText
				width: parent.width
				text: row.detailLine
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				elide: Text.ElideRight
			}
		}
		Item {
			id: tail
			anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
			width: 22
			height: 22

			Icon {
				anchors.centerIn: parent
				name: "check"
				size: 16
				color: Theme.accentText
				visible: row.isCurrent && !closeTap.visible
			}
			IconButton {
				id: closeTap
				anchors.fill: parent
				iconName: "close"
				iconSize: 16
				visible: row.isLive && row.current && !dialog.closing
				Accessible.name: qsTr("Close session")
				onClicked: Workspace.requestCloseSession(row.sessionId)
			}
		}
	}

	title: closing ? qsTr("Close a session") : qsTr("Switch session")
	onAccepted: list.activateCurrent()
	onFilterModeChanged: list.reset()

	// Search and creator filters run on the picker's own model, so the sidebar's list is not touched.
	Binding {
		target: Workspace.sessionChoices
		property: "filter"
		value: list.text.trim()
	}
	Binding {
		target: Workspace.sessionChoices
		property: "creatorFilter"
		value: dialog.creatorFilterValue
	}
	Binding {
		target: Workspace.sessionChoices
		property: "creatorId"
		value: dialog.lockedCreatorId
	}

	Column {
		width: parent.width

		Item {
			width: parent.width
			height: creatorTabs.height + 12

			Segmented {
				id: creatorTabs
				anchors { left: parent.left; right: parent.right; leftMargin: Theme.dialogPadding; rightMargin: Theme.dialogPadding; top: parent.top; topMargin: 4 }
				model: dialog.filterTabs
				value: dialog.filterMode
				onActivated: value => {
					if (value === "creator" && dialog.filterMode !== "creator") {
						dialog.lockedCreatorId = dialog.selectedRow ? dialog.selectedRow.parentId : ""
						dialog.lockedCreatorName = dialog.selectedRow ? dialog.selectedRow.creatorName : ""
					}
					dialog.filterMode = value
				}
			}

			Rectangle {
				anchors.bottom: parent.bottom
				width: parent.width
				height: 1
				color: Theme.border
			}
		}

		PickList {
			id: list
			width: parent.width
			placeholderText: qsTr("Search sessions")
			sectionProperty: "isLive"
			sectionLabel: section => section === dialog.liveSection ? qsTr("Running") : qsTr("History")
			emptyText: qsTr("No matching sessions")
			initialIndex: {
				if (dialog.closing)
					return -1
				void Workspace.sessionChoices.count
				return Workspace.sessionChoices.indexOf(Workspace.currentId)
			}
			rowEnabled: index => !dialog.closing || Workspace.sessionChoices.isLiveAt(index)
			model: Workspace.sessionChoices
			delegate: SessionPickRow {}
		}
	}
}
