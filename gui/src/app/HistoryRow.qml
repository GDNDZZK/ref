import QtQuick
import RefGui

// A past session in the sidebar: title, directory and when it was last used.
Item {
	id: row

	required property string sessionId
	required property string displayTitle
	required property string dirName
	required property double updatedAt
	required property string parentId
	required property string creatorName

	readonly property string creatorLabel: row.parentId !== "" ? (row.creatorName !== "" ? row.creatorName : row.parentId) : ""
	readonly property string detailText: creatorLabel !== ""
		? row.dirName + " · " + Format.relativeTime(row.updatedAt) + " · \u27f2 " + creatorLabel
		: row.dirName + " · " + Format.relativeTime(row.updatedAt)

	width: ListView.view ? ListView.view.width : 0
	height: Theme.sidebarHistoryRowHeight

	Rectangle {
		anchors { fill: parent; leftMargin: Theme.sidebarPadding; rightMargin: Theme.sidebarPadding }
		radius: Theme.radius
		color: hover.hovered ? Theme.surfaceHover : "transparent"
		scale: tap.pressed ? Motion.pressedScaleRow : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			textFormat: Text.PlainText
			x: 12
			y: 5
			width: parent.width - 2 * 12
			height: 18
			verticalAlignment: Text.AlignVCenter
			text: row.displayTitle
			color: Theme.text
			font.pixelSize: Theme.compactFontSize
			elide: Text.ElideRight
		}
		Text {
			textFormat: Text.PlainText
			x: 12
			y: 23
			width: parent.width - 2 * 12
			text: row.detailText
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			elide: Text.ElideRight
		}
	}

	HoverHandler {
		id: hover
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		onTapped: Workspace.openSession(row.sessionId)
	}
	TapHandler {
		acceptedButtons: Qt.RightButton
		onTapped: point => menu.openAt(point.position)
	}

	ContextMenu {
		id: menu
		entries: row.parentId !== ""
			? [
				{text: qsTr("Filter history by creator"), action: "filterCreator"},
				{text: qsTr("Filter spawned history"), action: "filterSpawned"},
				{text: qsTr("Delete"), danger: true, action: "delete"}
			]
			: [
				{text: qsTr("Filter spawned history"), action: "filterSpawned"},
				{text: qsTr("Delete"), danger: true, action: "delete"}
			]
		onTriggered: index => {
			const action = menu.entries[index].action
			if (action === "filterCreator")
				Workspace.filterHistoryByCreator(row.parentId)
			else if (action === "filterSpawned")
				Workspace.filterHistoryByCreator(row.sessionId)
			else
				Workspace.openDialog(DialogId.deleteHistory, {sessionId: row.sessionId, title: row.displayTitle})
		}
	}
}
