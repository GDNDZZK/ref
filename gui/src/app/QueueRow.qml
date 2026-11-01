import QtQuick
import QtQuick.Layouts
import RefGui

// One prompt in the QueueStrip: its text, what it carries, and a button to take it back.
Item {
	id: row

	required property int index
	required property string text
	required property int imageCount
	required property string sender
	required property bool removable
	required property bool removing
	property bool entered: false

	// Set by the strip.
	property int rowHeight: 0
	property int rowPadding: 0
	property int iconSize: 0
	property real removingOpacity: 1

	signal withdrawRequested()

	width: ListView.view.width
	height: entered ? row.rowHeight : 0
	opacity: entered ? (removing ? row.removingOpacity : 1) : 0
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
		anchors { fill: parent; leftMargin: row.rowPadding; rightMargin: row.rowPadding }
		spacing: 8

		Icon { name: "clock"; size: row.iconSize; color: Theme.textMuted }

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
			Icon { name: "image"; size: row.iconSize; color: Theme.textMuted }
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
				Icon { name: "recall"; size: row.iconSize; color: recallLabel.color }
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
			TapHandler { onTapped: row.withdrawRequested() }

			Accessible.role: Accessible.Button
			Accessible.name: qsTr("Remove from the queue")
			Accessible.onPressAction: row.withdrawRequested()
		}
	}
}
