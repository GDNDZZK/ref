pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// A checklist: a row per item {content, status} with a mark for the status. Used by the todo call's card, by the
// popup of the status line and by the sidebar. Long items are cut by default; with `wrap` they continue on further
// lines under the text, and the row grows.
Column {
	id: body

	required property var items
	property bool wrap: false

	readonly property int rowHeight: 26
	readonly property int wrapPadding: 4 // above and below a wrapped item
	property int leftPad: 14
	readonly property int markerSize: 16
	readonly property int verticalPadding: 4

	topPadding: verticalPadding
	bottomPadding: verticalPadding

	FontMetrics {
		id: metrics
		font.pixelSize: Theme.codeFontSize
	}

	Repeater {
		model: body.items

		delegate: Item {
			id: row

			required property var modelData
			readonly property bool completed: modelData.status === "completed"
			readonly property bool cancelled: modelData.status === "cancelled"
			readonly property bool active: modelData.status === "in_progress"

			width: body.width
			height: body.wrap ? Math.max(body.rowHeight, label.implicitHeight + 2 * body.wrapPadding) : body.rowHeight

			Item {
				id: marker
				x: body.leftPad
				// In line with the first line of the text.
				y: body.wrap ? label.y + (metrics.height - height) / 2 : (row.height - height) / 2
				width: body.markerSize
				height: body.markerSize

				// Waiting and working: a ring, with a pulsing dot inside while it is worked on.
				Rectangle {
					anchors.centerIn: parent
					width: marker.width - 2
					height: width
					radius: width / 2
					color: "transparent"
					border.width: 1.5
					border.color: row.active ? Theme.accent : Theme.textMuted
					opacity: row.completed || row.cancelled ? 0 : 1

					Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
				Rectangle {
					id: workingDot
					anchors.centerIn: parent
					width: 8
					height: 8
					radius: 4
					color: Theme.accent
					opacity: row.active ? 1 : 0

					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
				Pulse {
					target: workingDot
					running: row.active
					dimmed: 0.5
				}
				Rectangle {
					anchors.fill: parent
					radius: width / 2
					color: Theme.success
					opacity: row.completed ? 1 : 0

					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

					Icon {
						name: "check"
						color: "white"
					}
				}
				Icon {
					name: "close"
					color: Theme.textMuted
					opacity: row.cancelled ? 1 : 0

					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
			}

			Text {
				id: label
				textFormat: Text.PlainText
				anchors { left: marker.right; leftMargin: 8; right: parent.right; rightMargin: body.leftPad; verticalCenter: parent.verticalCenter }
				text: row.modelData.content
				wrapMode: body.wrap ? Text.Wrap : Text.NoWrap
				elide: body.wrap ? Text.ElideNone : Text.ElideRight
				color: row.completed || row.cancelled ? Theme.textMuted : Theme.text
				font.pixelSize: Theme.codeFontSize
				font.strikeout: row.completed || row.cancelled
			}
		}
	}
}
