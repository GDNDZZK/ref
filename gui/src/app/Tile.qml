import QtQuick
import RefGui

// One session in split view: a header with its state and the session pane below. The pane is
// the same item while the tile is zoomed, so scroll position and streaming survive.
Item {
	id: tile

	required property ChatSession session
	required property string sessionId
	required property bool leaving
	required property int place

	// Where the grid wants the tile, and whether it is the zoomed one.
	property rect slot: Qt.rect(0, 0, 0, 0)
	property rect zoomRect: Qt.rect(0, 0, 0, 0)
	// The tile of the current session starts as the whole area when split view opens, and
	// grows back to the whole area when it closes, in place of the single pane.
	property bool takeover: false
	readonly property bool growing: leaving && !Workspace.splitView && session !== null && session === Workspace.current
	readonly property bool zoomed: Workspace.zoomedId === sessionId
	readonly property bool expanded: zoomed || takeover || growing
	readonly property bool fading: leaving && !growing
	readonly property bool othersZoomed: Workspace.zoomedId !== "" && !zoomed
	// Whether the tile moves between its slot and the whole area (slowly) or just reflows.
	property bool travelling: expanded
	readonly property int travelDuration: travelling || leaving ? Motion.slow : Motion.standard
	readonly property bool focused: Workspace.focusedTileId === sessionId
	readonly property var facts: live.data
	readonly property bool hasPermission: facts.hasPermission === true
	readonly property bool failed: facts.hasError === true && facts.activity === "idle"
	property double now: Date.now()

	function focusPane() {
		pane.focusInput()
	}

	x: expanded ? zoomRect.x : slot.x
	y: expanded ? zoomRect.y : slot.y
	width: expanded ? zoomRect.width : slot.width
	height: expanded ? zoomRect.height : slot.height
	z: expanded ? 1 : fading ? -1 : 0
	opacity: othersZoomed || fading ? 0 : 1
	scale: othersZoomed || fading ? Motion.tileExitScale : 1
	visible: opacity > 0
	enabled: !leaving

	Behavior on x { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on y { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on width { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on height { NumberAnimation { duration: tile.travelDuration; easing.type: Motion.ease } }
	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: tile.fading ? Motion.exitEase : Motion.ease } }
	Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: tile.fading ? Motion.exitEase : Motion.ease } }

	// A tile that leaves stays where it was while it fades; the others reflow into its slot.
	onLeavingChanged: {
		if (leaving)
			slot = Qt.rect(slot.x, slot.y, slot.width, slot.height)
	}

	onExpandedChanged: {
		if (expanded) {
			travelling = true
			settle.stop()
		} else {
			settle.restart()
		}
	}

	Timer {
		id: settle
		interval: Motion.slow
		onTriggered: tile.travelling = false
	}

	// A tile arrives by fading and growing into its slot; the taking-over one by shrinking from the whole area.
	Component.onCompleted: {
		if (takeover)
			Qt.callLater(() => tile.takeover = false)
		else
			arrival.start()
	}
	ParallelAnimation {
		id: arrival
		NumberAnimation { target: frame; property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: frame; property: "scale"; from: Motion.tileEnterScale; to: 1; duration: Motion.standard; easing.type: Motion.ease }
	}

	LiveInfo {
		id: live
		sessionId: tile.sessionId
	}

	Timer {
		interval: 1000
		repeat: true
		running: tile.facts.busySince > 0 && tile.visible
		onTriggered: tile.now = Date.now()
	}

	Rectangle {
		id: frame
		anchors.fill: parent
		radius: Theme.radius
		color: Theme.background
		border.color: tile.focused || tile.expanded ? Theme.accent : Theme.border
		clip: true

		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		Rectangle {
			id: header
			width: parent.width
			height: tile.expanded ? 0 : Theme.tileHeaderHeight
			color: tile.hasPermission ? Theme.warningSurface : Theme.surface
			opacity: tile.expanded ? 0 : 1
			clip: true

			Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Rectangle {
				anchors.bottom: parent.bottom
				width: parent.width
				height: 1
				color: tile.hasPermission ? Theme.warningBorder : Theme.border
			}

			SessionDot {
				x: 12
				anchors.verticalCenter: parent.verticalCenter
				activity: tile.facts.activity ?? "idle"
				hasError: tile.facts.hasError ?? false
			}

			Text {
				anchors { left: parent.left; leftMargin: 30; right: status.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
				text: tile.facts.displayTitle ?? ""
				color: Theme.text
				font.pixelSize: Theme.fontSize - 2
				elide: Text.ElideRight
			}

			Row {
				id: status
				anchors { right: zoomButton.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
				spacing: 8

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: "lock"
					size: 16
					color: Theme.warning
					visible: tile.hasPermission
				}
				AlertBadge {
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: 14
					implicitHeight: 14
					visible: tile.failed
				}
				Text {
					anchors.verticalCenter: parent.verticalCenter
					visible: tile.facts.todoTotal > 0
					text: (tile.facts.todoAllDone ? "✓" : "") + tile.facts.todoDone + "/" + tile.facts.todoTotal
					color: tile.facts.todoAllDone ? Theme.success : Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
				Text {
					anchors.verticalCenter: parent.verticalCenter
					visible: tile.facts.busySince > 0
					text: Format.elapsed(tile.now - tile.facts.busySince)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
			}

			CircleButton {
				id: zoomButton
				anchors { right: parent.right; rightMargin: 4; verticalCenter: parent.verticalCenter }
				implicitWidth: 28
				implicitHeight: 28
				Accessible.name: qsTr("Zoom in")
				onClicked: Workspace.zoomTile(tile.sessionId)

				Icon {
					anchors.centerIn: parent
					name: "maximize"
					size: 20
					color: Theme.textMuted
				}
			}

			TapHandler {
				acceptedButtons: Qt.LeftButton
				onDoubleTapped: Workspace.zoomTile(tile.sessionId)
			}
		}

		SessionPane {
			id: pane
			anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
			session: tile.session
			compact: !tile.expanded
			keysEnabled: tile.focused || tile.expanded
		}

		// A click anywhere in the tile focuses it, and its input box.
		TapHandler {
			acceptedButtons: Qt.LeftButton
			gesturePolicy: TapHandler.ReleaseWithinBounds
			onTapped: {
				Workspace.focusTile(tile.sessionId)
				pane.focusInput()
			}
		}
	}

	Connections {
		target: Workspace
		function onFocusedTileChanged() {
			if (tile.focused)
				pane.focusInput()
		}
	}
}
