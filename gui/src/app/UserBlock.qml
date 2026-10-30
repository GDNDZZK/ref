// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// The user's message. A message that opens a turn sits below a divider that separates
// it from the previous reply; so do all but the first, except those the harness or a
// shell command added in the middle of a turn. Short messages hug their text.
Block {
	id: block

	required property bool opensTurn
	required property string source
	required property string sender

	readonly property bool opensLaterTurn: opensTurn
	// Says who a message is from when it is not what the user typed.
	readonly property string caption: {
		switch (source) {
		case "peer":
			return qsTr("From %1").arg(sender)
		case "spawn":
			return qsTr("Task from %1").arg(sender)
		case "shell":
			return qsTr("Shell command")
		case "reminder":
			return qsTr("Reminder")
		case "compaction":
			return qsTr("Summary of the earlier conversation")
		default:
			return ""
		}
	}

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

		Text {
			visible: block.caption !== ""
			text: block.caption
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
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
