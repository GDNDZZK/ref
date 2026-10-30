pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
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
	// Height of the visible rows; follows the rows while they come and go.
	readonly property real rowsHeight: Math.min(list.contentHeight, queueMaxRows * queueRowHeight)

	signal withdrawRequested(int row)

	implicitHeight: rowsHeight
	height: implicitHeight
	visible: height > 0
	radius: Theme.radius
	color: Theme.surface
	border.color: Theme.border
	clip: true

	ListView {
		id: list
		anchors.fill: parent
		model: strip.model
		interactive: contentHeight > height
		boundsBehavior: Flickable.StopAtBounds

		delegate: Item {
			id: row

			required property int index
			required property string text
			required property int imageCount
			required property string sender
			required property bool removable
			required property bool removing
			property bool entered: false

			width: ListView.view.width
			height: entered ? strip.queueRowHeight : 0
			opacity: entered ? (removing ? strip.removingOpacity : 1) : 0
			clip: true

			Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Component.onCompleted: entered = true
			// The row stays until the animation has folded it away.
			ListView.onRemove: removeAnimation.start()
			SequentialAnimation {
				id: removeAnimation
				PropertyAction { target: row; property: "ListView.delayRemove"; value: true }
				ScriptAction { script: row.entered = false }
				PauseAnimation { duration: Motion.standard }
				PropertyAction { target: row; property: "ListView.delayRemove"; value: false }
			}

			Rectangle {
				visible: row.index > 0
				width: parent.width
				height: 1
				color: Theme.border
			}

			RowLayout {
				anchors { fill: parent; leftMargin: strip.rowPadding; rightMargin: strip.rowPadding }
				spacing: 8

				Icon { name: "clock"; size: strip.iconSize; color: Theme.textMuted }

				Text {
					textFormat: Text.PlainText
					Layout.fillWidth: true
					text: row.text.replace(/\s+/g, " ")
					color: Theme.text
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}

				RowLayout {
					visible: row.imageCount > 0
					spacing: 4
					Icon { name: "image"; size: strip.iconSize; color: Theme.textMuted }
					Text {
						textFormat: Text.PlainText
						text: row.imageCount
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}
				}

				Text {
					textFormat: Text.PlainText
					visible: row.sender !== ""
					text: row.sender
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}

				Item {
					id: recall
					visible: row.removable
					enabled: !row.removing
					implicitWidth: recallRow.implicitWidth
					implicitHeight: recallRow.implicitHeight

					Row {
						id: recallRow
						spacing: 4
						Icon { name: "recall"; size: strip.iconSize; color: recallLabel.color }
						Text {
							id: recallLabel
							textFormat: Text.PlainText
							anchors.verticalCenter: parent.verticalCenter
							text: qsTr("Remove")
							color: recallHover.hovered ? Theme.text : Theme.textMuted
							font.pixelSize: Theme.smallFontSize

							Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
						}
					}

					HoverHandler {
						id: recallHover
						cursorShape: Qt.PointingHandCursor
					}
					TapHandler { onTapped: strip.withdrawRequested(row.index) }

					Accessible.role: Accessible.Button
					Accessible.name: qsTr("Remove from the queue")
					Accessible.onPressAction: strip.withdrawRequested(row.index)
				}
			}
		}
	}
}
