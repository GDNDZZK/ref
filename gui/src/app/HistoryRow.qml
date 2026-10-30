import QtQuick
import RefGui

// A past session in the sidebar: title, directory and when it was last used.
Item {
	id: row

	required property string sessionId
	required property string displayTitle
	required property string dirName
	required property double updatedAt

	width: ListView.view ? ListView.view.width : 0
	height: Theme.sidebarHistoryRowHeight

	Rectangle {
		anchors { fill: parent; leftMargin: Theme.sidebarPadding; rightMargin: Theme.sidebarPadding }
		radius: Theme.radius
		color: hover.hovered ? Theme.surfaceHover : "transparent"
		scale: tap.pressed ? 0.98 : 1

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
			font.pixelSize: Theme.fontSize - 2
			elide: Text.ElideRight
		}
		Text {
			textFormat: Text.PlainText
			x: 12
			y: 23
			width: parent.width - 2 * 12
			text: row.dirName + " · " + Format.relativeTime(row.updatedAt)
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
		entries: [{text: qsTr("Delete"), danger: true}]
		onTriggered: Workspace.openDialog("delete-history", {sessionId: row.sessionId, title: row.displayTitle})
	}
}
