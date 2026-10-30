import QtQuick
import Qutie

// The user's message. It opens a turn: every message but the first sits below a
// divider that separates it from the previous reply. Short messages hug their text.
Block {
	id: block

	required property int index

	readonly property bool opensLaterTurn: index > 0

	// Draws the divider across only for a freshly sent message.
	onAdded: if (opensLaterTurn) dividerGrow.restart()

	Column {
		width: block.columnWidth
		topPadding: block.opensLaterTurn ? Theme.turnGap : 0
		spacing: Theme.turnGap

		Rectangle {
			id: divider
			visible: block.opensLaterTurn
			width: parent.width
			height: 1
			color: Theme.border

			transform: Scale {
				id: dividerScale
				origin.x: 0
				xScale: 1
			}

			NumberAnimation {
				id: dividerGrow
				target: dividerScale
				property: "xScale"
				from: 0
				to: 1
				duration: Motion.slow
				easing.type: Motion.ease
			}
		}

		Rectangle {
			id: bubble

			readonly property int horizontalPadding: 14
			readonly property int verticalPadding: 9

			width: message.width + 2 * horizontalPadding
			height: message.height + 2 * verticalPadding
			radius: Theme.radiusLarge
			color: Theme.userBubble
			border.color: Theme.userBubbleBorder

			ProseText {
				id: message
				x: bubble.horizontalPadding
				y: bubble.verticalPadding
				width: Math.min(implicitWidth, block.columnWidth - 2 * bubble.horizontalPadding)
				textFormat: TextEdit.PlainText
				text: block.content
			}
		}
	}
}
