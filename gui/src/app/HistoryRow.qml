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
	required property bool pinned

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
		scale: openTap.pressed ? Motion.pressedScaleRow : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			id: title
			textFormat: Text.PlainText
			x: 12
			y: 5
			width: (pinButton.visible ? pinButton.x - 4 : parent.width - 12) - x
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
		Item {
			anchors { left: parent.left; top: parent.top; bottom: parent.bottom; right: pinButton.visible ? pinButton.left : parent.right }

			TapHandler {
				id: openTap
				onTapped: Workspace.openSession(row.sessionId)
			}
		}
		IconButton {
			id: pinButton
			anchors { right: parent.right; rightMargin: 10; verticalCenter: title.verticalCenter }
			buttonSize: 22
			iconName: "pin"
			iconSize: 16
			iconColor: row.pinned ? Theme.accentText : Theme.textMuted
			opacity: row.pinned || hover.hovered ? 1 : 0
			visible: opacity > 0
			tooltipText: row.pinned ? qsTr("Unpin") : qsTr("Pin")
			Accessible.name: row.pinned ? qsTr("Unpin") : qsTr("Pin")
			onClicked: Workspace.setSessionPinned(row.sessionId, !row.pinned)

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}
	}

	HoverHandler {
		id: hover
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		acceptedButtons: Qt.RightButton
		onTapped: point => menu.openAt(point.position)
	}

	ContextMenu {
		id: menu
		entries: {
			const items = [
				{text: row.pinned ? qsTr("Unpin") : qsTr("Pin"), action: "pin"}
			]
			if (row.parentId !== "")
				items.push({text: qsTr("Filter history by creator"), action: "filterCreator"})
			items.push({text: qsTr("Filter spawned history"), action: "filterSpawned"})
			items.push({text: qsTr("Delete"), danger: true, action: "delete"})
			return items
		}
		onTriggered: index => {
			const action = menu.entries[index].action
			if (action === "pin")
				Workspace.setSessionPinned(row.sessionId, !row.pinned)
			else if (action === "filterCreator")
				Workspace.filterHistoryByCreator(row.parentId)
			else if (action === "filterSpawned")
				Workspace.filterHistoryByCreator(row.sessionId)
			else
				Workspace.openDialog(DialogId.deleteHistory, {sessionId: row.sessionId, title: row.displayTitle})
		}
	}
}
