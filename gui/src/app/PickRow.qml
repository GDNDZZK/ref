import QtQuick
import RefGui

// Base of the rows of a PickList. Hovering selects the row, a click or Enter activates it.
Item {
	id: row

	required property int index
	readonly property bool current: ListView.isCurrentItem
	// Draws the check mark at the right end; content ends at check.left.
	property bool selected: false
	readonly property alias check: checkIcon

	signal activated()

	width: ListView.view ? ListView.view.width : 0
	height: Theme.pickRowHeight
	opacity: enabled ? 1 : Theme.disabledOpacity

	Icon {
		id: checkIcon
		anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
		name: "check"
		size: 16
		color: Theme.accentText
		visible: row.selected
	}

	HoverHandler {
		enabled: row.enabled
		cursorShape: Qt.PointingHandCursor
		onHoveredChanged: if (hovered && row.ListView.view) row.ListView.view.currentIndex = row.index
	}
	TapHandler {
		enabled: row.enabled
		onTapped: row.activated()
	}
}
