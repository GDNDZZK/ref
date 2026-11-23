pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// The prompts that wait for the next safe point, above the input box. Prompts of the user can
// be taken back; messages of other sessions and reminders cannot.
Rectangle {
	id: strip

	readonly property int queueRowHeight: 32
	readonly property int queueMaxRows: 4
	readonly property int rowPadding: 12
	readonly property int iconSize: 16
	readonly property real removingOpacity: 0.5

	required property var model
	readonly property real targetHeight: Math.min(list.count, queueMaxRows) * queueRowHeight
	property bool ready: false

	signal withdrawRequested(int row)

	Component.onCompleted: ready = true

	implicitHeight: targetHeight
	height: implicitHeight
	visible: height > 0 || list.count > 0
	radius: Theme.radius
	color: Theme.surface
	border.color: Theme.border
	clip: true

	Behavior on implicitHeight {
		enabled: strip.ready
		NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
	}

	ListView {
		id: list
		width: parent.width
		height: Math.max(strip.queueRowHeight, strip.height)
		model: strip.model
		interactive: contentHeight > height
		boundsBehavior: Flickable.StopAtBounds

		delegate: QueueRow {
			rowHeight: strip.queueRowHeight
			rowPadding: strip.rowPadding
			iconSize: strip.iconSize
			removingOpacity: strip.removingOpacity
			onWithdrawRequested: strip.withdrawRequested(index)
		}
	}
}
