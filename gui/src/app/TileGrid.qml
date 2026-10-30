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
	// Whether any tile is on screen, also one that is leaving.
	readonly property bool occupied: repeater.count > 0
	readonly property int padding: Theme.tilePadding
	readonly property int gap: Theme.tileGap
	// Room kept free at the top for the window buttons.
	property int topInset: 0
	readonly property int contentTop: padding + topInset
	readonly property int fit: Math.max(1, Math.floor((width - 2 * padding + gap) / (Theme.tileMinWidth + gap)))
	readonly property int columns: Math.max(1, Math.min(Math.ceil(Math.sqrt(count)), fit))
	readonly property int rows: Math.ceil(count / columns)
	readonly property real tileHeight: Math.max(Theme.tileMinHeight, (height - contentTop - padding - (rows - 1) * gap) / rows)
	readonly property real gridHeight: contentTop + padding + rows * tileHeight + (rows - 1) * gap

	clip: true

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
		if (Workspace.zoomedId !== "")
			return
		let from = null
		for (let i = 0; i < repeater.count; ++i) {
			if ((repeater.itemAt(i) as Tile).focused)
				from = repeater.itemAt(i) as Tile
		}
		if (from === null)
			return
		const crossWeight = 2
		let best = null
		let bestCost = Infinity
		for (let i = 0; i < repeater.count; ++i) {
			const tile = repeater.itemAt(i) as Tile
			const along = ((tile.slot.x + tile.slot.width / 2) - (from.slot.x + from.slot.width / 2)) * dx
			              + ((tile.slot.y + tile.slot.height / 2) - (from.slot.y + from.slot.height / 2)) * dy
			if (tile === from || tile.leaving || along <= 0)
				continue
			const across = Math.abs(((tile.slot.x + tile.slot.width / 2) - (from.slot.x + from.slot.width / 2)) * dy
			                        + ((tile.slot.y + tile.slot.height / 2) - (from.slot.y + from.slot.height / 2)) * dx)
			const cost = along + crossWeight * across
			if (cost < bestCost) {
				best = tile
				bestCost = cost
			}
		}
		if (best !== null)
			Workspace.focusTile(best.sessionId)
	}

	Flickable {
		id: flick
		anchors.fill: parent
		contentWidth: width
		contentHeight: Workspace.zoomedId === "" ? grid.gridHeight : height
		interactive: Workspace.zoomedId === ""
		boundsBehavior: Flickable.StopAtBounds

		ScrollBar.vertical: ThemedScrollBar {}

		Connections {
			target: Workspace
			function onZoomedChanged() {
				if (Workspace.zoomedId !== "")
					flick.contentY = 0
			}
			// The tile that grows back to the whole area starts from the top.
			function onSplitViewChanged() {
				if (!Workspace.splitView)
					flick.contentY = 0
			}
		}

		Repeater {
			id: repeater
			model: Workspace.tileStage

			delegate: Tile {
				id: tile

				readonly property int row: Math.floor(place / grid.columns)
				readonly property int inRow: row === grid.rows - 1 ? grid.count - row * grid.columns : grid.columns
				readonly property real slotWidth: (grid.width - 2 * grid.padding - (inRow - 1) * grid.gap) / inRow

				slot: Qt.rect(grid.padding + (place - row * grid.columns) * (slotWidth + grid.gap),
				              grid.contentTop + row * (grid.tileHeight + grid.gap),
				              slotWidth,
				              grid.tileHeight)
				zoomRect: Qt.rect(0, 0, grid.width, grid.height)
				takeover: Workspace.tileStage.opening && session === Workspace.current
			}
		}
	}

	Column {
		anchors.centerIn: parent
		spacing: 12
		visible: grid.count === 0 && Workspace.splitView && Workspace.current === null

		Text {
			textFormat: Text.PlainText
			anchors.horizontalCenter: parent.horizontalCenter
			text: qsTr("No background sessions")
			color: Theme.textMuted
			font.pixelSize: Theme.fontSize
		}
		TextButton {
			anchors.horizontalCenter: parent.horizontalCenter
			text: qsTr("New session")
			onClicked: Commands.run("new")
		}
	}
}
