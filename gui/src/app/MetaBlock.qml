import QtQuick
import QtQuick.Controls.Basic
import RefGui

// The line under a message: for the user's, the time and the actions; for the reply, who
// wrote it, when, how many tokens it cost and the actions. Quiet until the pointer is over it.
Block {
	id: block

	required property string messageId
	required property string source
	required property real time
	required property real timeEnd
	required property string agent
	required property string modelId
	required property string variant
	required property real tokensIn
	required property real tokensOut
	required property bool interrupted
	required property bool errored
	required property bool human

	readonly property bool fromUser: source !== ""
	readonly property real restingOpacity: 0.6
	readonly property int rowHeight: 24
	readonly property int itemSpacing: 8
	pullUp: 8 // brings the line closer to its message than rows are apart
	readonly property real replyTime: timeEnd > 0 ? timeEnd - time : 0
	readonly property real tokensPerSecond: replyTime > 0 ? tokensOut / (replyTime / Format.msPerSecond) : 0
	readonly property int maxPlausibleTokensPerSecond: 100000
	readonly property bool working: !fromUser && timeEnd === 0 && !errored && !interrupted
	// The agent's color by its place in the name's hash, until the session knows the agent list.
	readonly property color agentColor: {
		let hash = 0
		for (let i = 0; i < agent.length; ++i)
			hash = (hash * 31 + agent.charCodeAt(i)) % 1000003
		return Theme.agentColor("", hash)
	}
	readonly property string agentTitle: agent === "" ? "" : agent.charAt(0).toUpperCase() + agent.slice(1)

	property real now: Date.now()
	property bool copied: false

	Timer {
		interval: Format.msPerSecond
		running: block.working
		repeat: true
		onTriggered: block.now = Date.now()
	}
	Timer {
		id: copiedTimer
		interval: Motion.slow * 3
		onTriggered: block.copied = false
	}

	Item {
		id: row
		width: block.columnWidth
		height: block.rowHeight
		opacity: hover.hovered ? 1 : block.restingOpacity

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		HoverHandler { id: hover }

		// The reply's facts, left.
		Row {
			visible: !block.fromUser
			anchors { left: parent.left; verticalCenter: parent.verticalCenter }
			spacing: block.itemSpacing

			Rectangle {
				id: agentDot
				anchors.verticalCenter: parent.verticalCenter
				width: 8
				height: 8
				radius: 4
				color: block.errored ? Theme.textMuted : block.agentColor

				Pulse { target: agentDot; running: block.working; dimmed: 0.5 }
			}
			MetaText {
				text: block.agentTitle
				color: agentDot.color
				font.bold: true
				visible: text !== ""
			}
			MetaText {
				text: block.variant === "" ? block.modelId : block.modelId + " "
				visible: block.modelId !== ""
				MetaText {
					anchors.left: parent.right
					text: block.variant
					color: Theme.warning
				}
			}
			MetaText {
				id: timeText
				text: block.timeEnd > 0
					? Format.clock(block.time) + " \u2192 " + Format.clock(block.timeEnd) + " (" + Format.duration(block.replyTime) + ")"
					: Format.clock(block.time) + " \u00b7 " + Format.elapsed(block.now - block.time)
				ToolTip { text: new Date(block.time).toLocaleString(); visible: timeHover.hovered }
				HoverHandler { id: timeHover }
			}
			MetaText {
				text: "\u2191" + Format.tokens(block.tokensIn) + " \u2193" + Format.tokens(block.tokensOut)
				visible: block.tokensIn + block.tokensOut > 0
			}
			MetaText {
				text: Math.round(block.tokensPerSecond) + " tok/s"
				opacity: block.restingOpacity
				visible: block.tokensOut > 0 && block.tokensPerSecond > 0 && block.tokensPerSecond < block.maxPlausibleTokensPerSecond
			}
			MetaText {
				text: qsTr("interrupted")
				color: Theme.warning
				visible: block.interrupted
			}
		}

		// The actions, right.
		Row {
			anchors { right: parent.right; verticalCenter: parent.verticalCenter }
			spacing: block.itemSpacing

			MetaText {
				visible: block.fromUser
				text: Format.clock(block.time)
			}
			MetaAction {
				visible: block.human
				iconName: "undo"
				label: qsTr("Undo")
				onTriggered: block.ListView.view.undoRequested(block.messageId)
			}
			MetaAction {
				iconName: block.copied ? "check" : "copy"
				label: qsTr("Copy")
				onTriggered: {
					block.ListView.view.copyRequested(block.messageId)
					block.copied = true
					copiedTimer.restart()
				}
			}
		}
	}
}
