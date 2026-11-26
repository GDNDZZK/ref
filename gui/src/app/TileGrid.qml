pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// Split view: the tiles on a grid. Columns follow the width (at most as many as fit at the
// minimum tile width, else about square); a partly filled last row shares its width. When the
// grid is taller than the area it scrolls.
Item {
	id: grid

	readonly property int count: Workspace.tileStage.placed
	property string lastZoomedId: ""
	readonly property string activeZoomedId: Workspace.splitView ? Workspace.zoomedId : lastZoomedId
	readonly property bool zoomed: activeZoomedId !== ""
	// Whether any tile is on screen, also one that is leaving.
	readonly property bool occupied: repeater.count > 0
	readonly property int padding: Theme.tilePadding
	readonly property int gap: Theme.tileGap
	// Room kept free at the top for the window buttons.
	property int topInset: 0
	readonly property int contentTop: padding + topInset
	property bool sidebarAnimating: false
	property real minAreaWidth: width
	readonly property real stableWidth: sidebarAnimating ? minAreaWidth : width
	readonly property int fit: Math.max(1, Math.floor((width - 2 * padding + gap) / (Theme.tileMinWidth + gap)))
	readonly property int columns: Math.max(1, Math.min(Math.ceil(Math.sqrt(count)), fit))
	readonly property int rows: Math.ceil(count / columns)
	readonly property real tileHeight: rows === 0 ? 0 : Math.max(Theme.tileMinHeight, (height - contentTop - padding - (rows - 1) * gap) / rows)
	readonly property real gridHeight: rows === 0 ? 0 : contentTop + padding + rows * tileHeight + (rows - 1) * gap

	clip: true
	opacity: Workspace.splitView ? 1 : 0

	Behavior on opacity {
		NumberAnimation {
			duration: Motion.standard
			easing.type: Workspace.splitView ? Motion.ease : Motion.exitEase
		}
	}
	// Tiles that leave stay until their exit animation is over.
	Binding {
		target: Workspace.tileStage
		property: "lingerMs"
		value: Motion.slow
	}

	// Puts the keyboard in the input box of the focused tile.
	function focusFocused() {
		for (let i = 0; i < repeater.count; ++i) {
			const tile = repeater.itemAt(i) as Tile
			if (tile.focused)
				tile.focusPane()
		}
	}

	// Focuses the nearest tile in the direction (dx, dy), each -1, 0 or 1. Distance counts
	// twice as much across the direction as along it, so a neighbour in line wins.
	function moveFocus(dx, dy) {
		if (zoomed)
			return
		let from = null
		for (let i = 0; i < repeater.count; ++i) {
			if ((repeater.itemAt(i) as Tile).focused)
				from = repeater.itemAt(i) as Tile
		}
		if (from === null) {
			for (let i = 0; i < repeater.count; ++i) {
				const tile = repeater.itemAt(i) as Tile
				if (!tile.leaving) {
					Workspace.focusTile(tile.sessionId)
					return
				}
			}
			return
		}
		const crossWeight = 2
		let best = null
		let bestCost = Infinity
		for (let i = 0; i < repeater.count; ++i) {
			const tile = repeater.itemAt(i) as Tile
			const deltaX = (tile.placedSlot.x + tile.placedSlot.width / 2) - (from.placedSlot.x + from.placedSlot.width / 2)
			const deltaY = (tile.placedSlot.y + tile.placedSlot.height / 2) - (from.placedSlot.y + from.placedSlot.height / 2)
			const along = deltaX * dx + deltaY * dy
			if (tile === from || tile.leaving || along <= 0)
				continue
			const across = Math.abs(deltaX * dy + deltaY * dx)
			const cost = along + crossWeight * across
			if (cost < bestCost) {
				best = tile
				bestCost = cost
			}
		}
		if (best !== null)
			Workspace.focusTile(best.sessionId)
	}

	Component.onCompleted: lastZoomedId = Workspace.zoomedId

	Flickable {
		id: flick
		anchors.fill: parent
		contentWidth: width
		contentHeight: grid.zoomed ? Math.max(grid.gridHeight, flick.contentY + height) : grid.gridHeight
		interactive: !grid.zoomed
		boundsBehavior: Flickable.StopAtBounds

		ScrollBar.vertical: ThemedScrollBar { visible: !grid.zoomed }

		Connections {
			target: Workspace
			function onZoomedChanged() {
				if (Workspace.splitView)
					grid.lastZoomedId = Workspace.zoomedId
				if (!grid.zoomed)
					flick.contentY = Math.min(flick.contentY, Math.max(0, grid.gridHeight - flick.height))
			}
			function onSplitViewChanged() {
				if (Workspace.splitView) {
					grid.lastZoomedId = Workspace.zoomedId
					flick.contentY = 0
				}
			}
		}

		Repeater {
			id: repeater
			model: Workspace.tileStage

			delegate: Tile {
				id: tile

				readonly property int row: Math.floor(place / grid.columns)
				readonly property int inRow: Math.max(1, row === grid.rows - 1 ? grid.count - row * grid.columns : grid.columns)
				readonly property real slotWidth: (grid.width - 2 * grid.padding - (inRow - 1) * grid.gap) / inRow
				readonly property int stableFit: Math.max(1, Math.floor((grid.stableWidth - 2 * grid.padding + grid.gap) / (Theme.tileMinWidth + grid.gap)))
				readonly property int stableColumns: Math.max(1, Math.min(Math.ceil(Math.sqrt(grid.count)), stableFit))
				readonly property int stableRows: Math.ceil(grid.count / stableColumns)
				readonly property int stableRow: Math.floor(place / stableColumns)
				readonly property int stableInRow: Math.max(1, stableRow === stableRows - 1 ? grid.count - stableRow * stableColumns : stableColumns)
				readonly property real stableSlotWidth: (grid.stableWidth - 2 * grid.padding - (stableInRow - 1) * grid.gap) / stableInRow

				slot: Qt.rect(grid.padding + (place - row * grid.columns) * (slotWidth + grid.gap),
				              grid.contentTop + row * (grid.tileHeight + grid.gap),
				              slotWidth,
				              grid.tileHeight)
				zoomed: grid.activeZoomedId === tile.sessionId
				anyZoomed: grid.zoomed
				sidebarAnimating: grid.sidebarAnimating
				stableWidth: tile.expanded ? grid.stableWidth : stableSlotWidth
				zoomRect: Qt.rect(0, flick.contentY, grid.width, grid.height)
			}
		}

		TapHandler {
			acceptedButtons: Qt.LeftButton
			enabled: Workspace.focusedTileId !== "" && !grid.zoomed
			onTapped: eventPoint => {
				const p = flick.mapToItem(flick.contentItem, eventPoint.position)
				for (let i = 0; i < repeater.count; ++i) {
					const tile = repeater.itemAt(i) as Tile
					if (!tile.leaving && p.x >= tile.x && p.x <= tile.x + tile.width && p.y >= tile.y && p.y <= tile.y + tile.height)
						return
				}
				Workspace.focusTile("")
			}
		}
	}

	Column {
		anchors.centerIn: parent
		spacing: 12
		opacity: grid.count === 0 && Workspace.splitView ? 1 : 0
		visible: opacity > 0

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Text {
			textFormat: Text.PlainText
			anchors.horizontalCenter: parent.horizontalCenter
			text: Workspace.splitFilter !== "all" ? qsTr("No matching background sessions") : qsTr("No background sessions")
			color: Theme.textMuted
			font.pixelSize: Theme.fontSize
		}
		Row {
			anchors.horizontalCenter: parent.horizontalCenter
			spacing: 8

			TextButton {
				visible: Workspace.splitFilter !== "all"
				text: qsTr("Show all")
				onClicked: Workspace.setSplitFilter("all")
			}
			TextButton {
				text: qsTr("New session")
				onClicked: Commands.run(CommandId.newSession)
			}
		}
	}
}
