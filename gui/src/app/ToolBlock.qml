import QtQuick
import Qutie

// A tool call: status, title and, on demand, its output.
Block {
	id: block

	required property string output
	required property bool failed

	property bool expanded: false
	readonly property bool hasOutput: output !== ""

	Rectangle {
		id: card
		width: block.columnWidth
		height: header.height + body.height
		radius: Theme.radius
		color: hover.hovered && block.hasOutput ? Theme.surfaceHover : Theme.surface
		border.color: Theme.border

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		// Stays in view while the output scrolls past under it (Block.pinOffset).
		Item {
			id: header

			readonly property real offset: block.pinOffset(height)

			y: offset
			z: 1 // above the output
			width: parent.width
			height: 38

			// Hides the output under the pinned header. Inside the card's border; its
			// corners follow the card's where the header nears the card's top or end.
			Rectangle {
				readonly property real inner: card.radius - card.border.width
				readonly property real toEnd: card.height - header.offset - header.height

				anchors {
					fill: parent
					leftMargin: card.border.width
					rightMargin: card.border.width
					topMargin: Math.max(0, card.border.width - header.offset)
					bottomMargin: Math.max(0, card.border.width - toEnd)
				}
				topLeftRadius: Math.max(0, inner - header.offset)
				topRightRadius: topLeftRadius
				bottomLeftRadius: Math.max(0, inner - toEnd)
				bottomRightRadius: bottomLeftRadius
				color: card.color

				Rectangle {
					anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
					height: 1
					color: Theme.border
					visible: header.offset > 0 && parent.toEnd > 0 // at rest, the body's divider is here
				}
			}

			StatusDot {
				id: dot
				anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
				running: block.streaming
				failed: block.failed
			}

			Text {
				anchors {
					left: dot.right
					leftMargin: 10
					right: chevron.left
					rightMargin: 8
					verticalCenter: parent.verticalCenter
				}
				text: block.content
				elide: Text.ElideRight
				color: Theme.text
				font.family: Theme.monoFamily
				font.pixelSize: Theme.codeFontSize
			}

			Chevron {
				id: chevron
				anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
				rotation: block.expanded ? 90 : 0
				opacity: block.hasOutput ? 1 : 0

				Behavior on rotation { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			}

			HoverHandler {
				id: hover
				cursorShape: block.hasOutput ? Qt.PointingHandCursor : Qt.ArrowCursor
			}
			TapHandler {
				enabled: block.hasOutput
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
			height: block.expanded ? outputText.height : 0
			clip: true

			Behavior on height {
				NumberAnimation {
					duration: Motion.standard
					easing.type: Motion.ease
					onRunningChanged: if (!running) block.releaseHeader()
				}
			}

			// While the body is shorter than the card's corner radius, the divider lies
			// within the rounded bottom corners; inset it to the arc so its ends do not
			// stick out past them. Collapsed, it matches the straight part of the bottom edge.
			Rectangle {
				readonly property real cornerRadius: Theme.radius
				readonly property real belowCornerCenter: Math.max(0, cornerRadius - body.height)
				readonly property real inset: cornerRadius - Math.sqrt(cornerRadius * cornerRadius - belowCornerCenter * belowCornerCenter)

				x: inset
				width: parent.width - 2 * inset
				height: 1
				color: Theme.border
			}

			TextEdit {
				id: outputText
				width: parent.width
				topPadding: 10
				bottomPadding: 12
				leftPadding: 14
				rightPadding: 14
				readOnly: true
				selectByMouse: true
				wrapMode: TextEdit.Wrap
				textFormat: TextEdit.PlainText
				text: block.output
				color: Theme.textMuted
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font.family: Theme.monoFamily
				font.pixelSize: Theme.smallFontSize
			}
		}
	}
}
