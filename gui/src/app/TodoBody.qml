import QtQuick
import RefGui

// The checklist of a todo call. Always shown: the plan is the point of the call.
Column {
	id: body

	required property var call

	signal showAllRequested()

	readonly property int rowHeight: 26
	readonly property int leftPad: 14
	readonly property int markerSize: 16
	readonly property int verticalPadding: 4
	readonly property var items: ToolInfo.todos(call.inputObject)

	topPadding: verticalPadding
	bottomPadding: verticalPadding

	Repeater {
		model: body.items

		delegate: Item {
			id: row

			required property var modelData
			readonly property bool completed: modelData.status === "completed"
			readonly property bool cancelled: modelData.status === "cancelled"
			readonly property bool active: modelData.status === "in_progress"

			width: body.width
			height: body.rowHeight

			Item {
				id: marker
				x: body.leftPad
				anchors.verticalCenter: parent.verticalCenter
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
				textFormat: Text.PlainText
				anchors { left: marker.right; leftMargin: 8; right: parent.right; rightMargin: body.leftPad; verticalCenter: parent.verticalCenter }
				text: row.modelData.content
				elide: Text.ElideRight
				color: row.completed || row.cancelled ? Theme.textMuted : Theme.text
				font.pixelSize: Theme.codeFontSize
				font.strikeout: row.completed || row.cancelled
			}
		}
	}
}
