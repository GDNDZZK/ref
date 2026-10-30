// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// A user-role message. A message that opens a turn sits below a divider that separates
// it from the previous reply; so do all but the first, except those the harness or a
// shell command added in the middle of a turn. Short messages hug their text.
//
// What the message is shows in its bubble: the user's own is tinted, messages from other
// sessions carry an info bar and a header, reminders and shell output a warning bar. The
// summary that replaces the history after a compaction is a divider with a pill that
// unfolds it.
Block {
	id: block

	required property string messageId
	required property bool opensTurn
	required property string source
	required property string sender
	required property real time
	required property bool expanded
	required property bool contextStart

	readonly property bool opensLaterTurn: opensTurn
	readonly property bool isCompaction: source === "compaction"
	readonly property bool hasMarker: source !== "human" && !isCompaction
	readonly property int markerBarWidth: 3
	readonly property int markerGap: 10
	readonly property int headerGap: 6

	// Who the message is from, as its header says it.
	readonly property string markerTitle: {
		switch (source) {
		case "peer":
			return qsTr("message from \"%1\"").arg(sender)
		case "spawn":
			return qsTr("created by \"%1\"").arg(sender)
		case "shell":
			return qsTr("shell")
		case "reminder":
			return qsTr("reminder")
		default:
			return ""
		}
	}
	readonly property string markerIcon: {
		switch (source) {
		case "peer":
			return "chat-send"
		case "spawn":
			return "chat-plus"
		case "shell":
			return "terminal"
		case "reminder":
			return "retry"
		default:
			return ""
		}
	}
	readonly property color markerColor: source === "peer" || source === "spawn" ? Theme.info : Theme.warning

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

		// The compaction divider: a line on both sides of a pill.
		Item {
			id: compaction

			readonly property int pillHeight: 28
			readonly property int pillPadding: 12

			visible: block.isCompaction
			width: parent.width
			height: pillHeight

			Rectangle {
				anchors { left: parent.left; right: pill.left; verticalCenter: parent.verticalCenter }
				height: 1
				color: Theme.border
			}
			Rectangle {
				anchors { left: pill.right; right: parent.right; verticalCenter: parent.verticalCenter }
				height: 1
				color: Theme.border
			}

			Rectangle {
				id: pill
				anchors.centerIn: parent
				width: pillRow.width + 2 * compaction.pillPadding
				height: compaction.pillHeight
				radius: height / 2
				color: pillHover.hovered ? Theme.surfaceHover : Theme.background
				border.color: Theme.border

				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Row {
					id: pillRow
					anchors.centerIn: parent
					spacing: 6

					Icon {
						anchors.verticalCenter: parent.verticalCenter
						name: "history"
						size: 16
						color: Theme.accent
					}
					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						text: qsTr("Context compacted")
						color: Theme.accent
						font.pixelSize: Theme.fontSize - 2
						font.bold: true
					}
					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						text: Format.clock(block.time)
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}
					Chevron {
						anchors.verticalCenter: parent.verticalCenter
						rotation: block.expanded ? 90 : 0
						Behavior on rotation { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
					}
				}

				HoverHandler { id: pillHover; cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: block.toggleExpanded() }
				Accessible.role: Accessible.Button
				Accessible.name: qsTr("Context compacted")
			}

			MetaAction {
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				iconName: "copy"
				label: qsTr("Copy")
				opacity: pillHover.hovered || hovered ? 1 : 0
				onTriggered: block.ListView.view.copyRequested(block.messageId)

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}

		// The summary, unfolded under the divider.
		Item {
			id: summary

			readonly property int barWidth: 2
			readonly property int textGap: 12

			visible: block.isCompaction
			width: parent.width
			height: block.expanded ? summaryText.height : 0
			clip: true

			Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Rectangle {
				width: summary.barWidth
				height: summaryText.height
				radius: width / 2
				color: Theme.accent
			}
			ProseText {
				id: summaryText
				x: summary.barWidth + summary.textGap
				width: parent.width - x
				textFormat: TextEdit.MarkdownText
				text: block.isCompaction ? block.content : ""
				color: Theme.textMuted
				font.pixelSize: Theme.fontSize - 2
			}
		}

		Text {
			textFormat: Text.PlainText
			visible: block.isCompaction && block.contextStart
			width: parent.width
			horizontalAlignment: Text.AlignHCenter
			text: qsTr("Messages above stay visible but are no longer sent to the model")
			color: Theme.textMuted
			opacity: 0.7
			font.pixelSize: Theme.smallFontSize
			font.italic: true
			wrapMode: Text.Wrap
		}

		Rectangle {
			id: bubble

			readonly property int horizontalPadding: 14
			readonly property int verticalPadding: 9
			readonly property real markerIndent: block.hasMarker ? block.markerBarWidth + block.markerGap : 0
			readonly property real textWidth: Math.max(message.width, marker.visible ? marker.width : 0)

			visible: !block.isCompaction
			width: markerIndent + textWidth + 2 * horizontalPadding
			height: (marker.visible ? marker.height + block.headerGap : 0) + message.height + 2 * verticalPadding
			radius: Theme.radiusLarge
			color: block.source === "shell" ? Theme.codeBackground : block.hasMarker ? Theme.surface : Theme.userBubble
			border.color: block.hasMarker ? Theme.border : Theme.userBubbleBorder

			Rectangle {
				visible: block.hasMarker
				x: bubble.horizontalPadding
				y: bubble.verticalPadding
				width: block.markerBarWidth
				height: bubble.height - 2 * bubble.verticalPadding
				radius: width / 2
				color: block.markerColor
			}

			Row {
				id: marker
				visible: block.hasMarker
				x: bubble.horizontalPadding + bubble.markerIndent
				y: bubble.verticalPadding
				spacing: 6

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: block.markerIcon
					size: 16
					color: block.markerColor
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					text: block.markerTitle
					color: block.markerColor
					font.pixelSize: Theme.smallFontSize
					font.bold: true
				}
			}

			ProseText {
				id: message
				x: bubble.horizontalPadding + bubble.markerIndent
				y: bubble.verticalPadding + (marker.visible ? marker.height + block.headerGap : 0)
				width: Math.min(implicitWidth, block.columnWidth - 2 * bubble.horizontalPadding - bubble.markerIndent)
				textFormat: block.source === "shell" ? TextEdit.MarkdownText : TextEdit.PlainText
				text: block.content
				font.family: block.source === "shell" ? Theme.monoFamily : font.family
			}
		}
	}
}
