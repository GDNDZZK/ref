import QtQuick
import RefGui

// What the session is doing, how long this has gone on, and how full its context window is.
// Always there, empty when idle, so that the conversation above does not shift.
Item {
	id: line

	required property ChatSession session

	readonly property int lineHeight: 28
	readonly property int itemSpacing: 8
	readonly property int barWidth: 48
	readonly property int barHeight: 4
	readonly property string activity: session.activity
	readonly property bool working: activity === "thinking" || activity === "running" || activity === "compacting"
	readonly property bool retrying: activity === "retrying"
	readonly property bool waiting: activity === "waiting_permission"
	readonly property real contextUsed: session.contextUsed
	readonly property real contextMax: session.contextMax
	readonly property real contextRatio: contextMax > 0 ? Math.min(1, contextUsed / contextMax) : 0
	readonly property color contextColor: session.contextAuto > 0 && contextUsed >= session.contextAuto ? Theme.danger
		: session.contextRecommend > 0 && contextUsed >= session.contextRecommend ? Theme.warning
		: Theme.accent
	readonly property real busyMs: session.busySince > 0 ? now - session.busySince : 0
	readonly property real shownFromMs: Format.msPerSecond

	property real now: Date.now()

	readonly property string activityText: {
		switch (activity) {
		case "thinking":
			return qsTr("Working")
		case "running":
			return qsTr("Running tools")
		case "compacting":
			return qsTr("Compacting context\u2026")
		case "waiting_permission":
			return qsTr("Waiting for your permission")
		case "retrying":
			return qsTr("Retrying in %1 (attempt %2): %3")
				.arg(Format.elapsed(Math.max(0, session.retryAt - now) + Format.msPerSecond - 1))
				.arg(session.retryAttempt)
				.arg(ToolInfo.firstLine(session.retryError))
		default:
			return ""
		}
	}

	height: lineHeight

	Timer {
		interval: Format.msPerSecond / 4
		running: line.activity !== "idle"
		repeat: true
		onTriggered: line.now = Date.now()
	}

	Row {
		anchors { left: parent.left; right: context.left; rightMargin: line.itemSpacing; verticalCenter: parent.verticalCenter }
		spacing: line.itemSpacing
		clip: true

		Rectangle {
			id: dot
			anchors.verticalCenter: parent.verticalCenter
			visible: line.working
			width: 8
			height: 8
			radius: 4
			color: Theme.accent

			Pulse { target: dot; running: line.working; dimmed: 0.5 }
		}
		Icon {
			anchors.verticalCenter: parent.verticalCenter
			visible: line.waiting || line.retrying
			name: line.waiting ? "lock" : "retry"
			color: Theme.warning
		}
		ShimmerText {
			anchors.verticalCenter: parent.verticalCenter
			visible: line.working
			label: line.activityText
			color: line.activity === "compacting" ? Theme.accent : Theme.textMuted
			running: visible
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			visible: line.waiting || line.retrying
			text: line.activityText
			color: Theme.warning
			elide: Text.ElideRight
			width: Math.min(implicitWidth, line.width / 2)
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			visible: line.activity !== "idle" && line.busyMs >= line.shownFromMs
			text: "\u00b7 " + Format.elapsed(line.busyMs)
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}

	Row {
		id: context
		anchors { right: parent.right; verticalCenter: parent.verticalCenter }
		spacing: line.itemSpacing
		visible: line.contextMax > 0

		Text {
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			text: Format.tokens(line.contextUsed) + " / " + Format.tokens(line.contextMax)
			color: line.contextColor === Theme.accent ? Theme.textMuted : line.contextColor
			font.pixelSize: Theme.smallFontSize
		}
		Rectangle {
			anchors.verticalCenter: parent.verticalCenter
			width: line.barWidth
			height: line.barHeight
			radius: height / 2
			color: Theme.border

			Rectangle {
				width: parent.width * line.contextRatio
				height: parent.height
				radius: parent.radius
				color: line.contextColor

				Behavior on width { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			}
		}

		HoverHandler { id: contextHover }
		PlainToolTip {
			visible: contextHover.hovered
			text: qsTr("Context: %1 of %2 tokens. Compaction is recommended at %3 and automatic at %4.")
				.arg(line.contextUsed).arg(line.contextMax).arg(line.session.contextRecommend).arg(line.session.contextAuto)
		}
	}
}
