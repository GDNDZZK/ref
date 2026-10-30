import QtQuick
import RefGui

// Base of the rows of a PickList. Hovering selects the row, a click or Enter activates it.
Item {
	id: row

	required property int index
	readonly property bool current: ListView.isCurrentItem

	signal activated()

	width: ListView.view ? ListView.view.width : 0
	height: 44
	opacity: enabled ? 1 : 0.4

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
