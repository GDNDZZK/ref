// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// The model's reasoning: open while it streams, folded away once it ends.
Block {
	id: block

	property bool expanded: streaming // follows streaming until the user toggles it

	Item {
		width: block.columnWidth
		height: header.height + body.height

		// Stays in view while the thoughts scroll past under it (Block.pinOffset).
		Item {
			id: header

			readonly property real offset: block.pinOffset(height)

			y: offset
			z: 1 // above the thoughts
			width: headerRow.width
			height: 28

			// Hides the thoughts under the pinned header, across the whole column.
			Rectangle {
				width: block.columnWidth
				height: parent.height
				color: Theme.background
				visible: header.offset > 0

				Rectangle {
					anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
					height: 1
					color: Theme.border
				}
			}

			Row {
				id: headerRow
				anchors.verticalCenter: parent.verticalCenter
				spacing: 6

				Chevron {
					anchors.verticalCenter: parent.verticalCenter
					rotation: block.expanded ? 90 : 0
					Behavior on rotation { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}

				Text {
					textFormat: Text.PlainText
					id: title
					anchors.verticalCenter: parent.verticalCenter
					text: block.streaming ? qsTr("Thinking") : qsTr("Thought")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize + 1

					Pulse {
						target: title
						running: block.streaming
						dimmed: 0.5
						period: Motion.calmPulse
					}
				}
			}

			HoverHandler { cursorShape: Qt.PointingHandCursor }
			TapHandler {
				onTapped: {
					if (block.expanded && header.offset > 0)
						block.holdHeader(header.height)
					block.expanded = !block.expanded
				}
			}
		}

		Item {
			id: body
			y: header.height
			width: parent.width
			height: block.expanded ? thoughts.height + 6 : 0
			clip: true

			Behavior on height {
				NumberAnimation {
					duration: Motion.standard
					easing.type: Motion.ease
					onRunningChanged: if (!running) block.releaseHeader()
				}
			}

			Rectangle {
				x: 5
				width: 2
				height: thoughts.height
				radius: 1
				color: Theme.border
			}

			ProseText {
				id: thoughts
				x: 18
				width: parent.width - x
				textFormat: TextEdit.MarkdownText
				text: block.content
				color: Theme.textMuted
				font.pixelSize: Theme.fontSize - 1
			}
		}
	}
}
