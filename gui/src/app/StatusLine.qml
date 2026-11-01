import QtQuick
import RefGui

// What the session is doing, how long this has gone on, how far its to-do list is, and how full its context window is.
// Always there, empty when idle, so that the conversation above does not shift.
Item {
	id: line

	// How full the context window is: past the size where compaction is recommended, or where it starts by itself.
	enum ContextLevel { Normal, Recommend, Auto }

	required property ChatSession session

	// The to-do popup was closed by Esc: the keyboard goes back to the pane.
	signal todoDismissed()

	readonly property int lineHeight: 28
	readonly property int itemSpacing: 8
	readonly property int barWidth: 48
	readonly property int barHeight: 4
	readonly property string activity: session.activity
	readonly property bool working: activity === Activity.thinking || activity === Activity.running || activity === Activity.compacting
	readonly property bool retrying: activity === Activity.retrying
	readonly property bool waiting: activity === Activity.waitingPermission || activity === Activity.waitingQuestion
	readonly property real contextUsed: session.contextUsed
	readonly property real contextMax: session.contextMax
	readonly property real contextRatio: contextMax > 0 ? Math.min(1, contextUsed / contextMax) : 0
	readonly property int contextLevel: {
		if (session.contextAuto > 0 && contextUsed >= session.contextAuto)
			return StatusLine.ContextLevel.Auto
		if (session.contextRecommend > 0 && contextUsed >= session.contextRecommend)
			return StatusLine.ContextLevel.Recommend
		return StatusLine.ContextLevel.Normal
	}
	readonly property color contextColor: contextLevel === StatusLine.ContextLevel.Auto ? Theme.danger
		: contextLevel === StatusLine.ContextLevel.Recommend ? Theme.warning
		: Theme.accent
	readonly property real busyMs: session.busySince > 0 ? ticker.now - session.busySince : 0
	readonly property real shownFromMs: Format.msPerSecond

	readonly property string activityText: {
		switch (activity) {
		case Activity.thinking:
			return qsTr("Working")
		case Activity.running:
			return qsTr("Running tools")
		case Activity.compacting:
			return qsTr("Compacting context\u2026")
		case Activity.waitingPermission:
			return qsTr("Waiting for your permission")
		case Activity.waitingQuestion:
			return qsTr("Waiting for your answer")
		case Activity.retrying:
			return qsTr("Retrying in %1 (attempt %2): %3")
				.arg(Format.countdown(session.retryAt - ticker.now))
				.arg(session.retryAttempt)
				.arg(ToolInfo.firstLine(session.retryError))
		default:
			return ""
		}
	}

	height: lineHeight

	Ticker {
		id: ticker
		interval: Format.msPerSecond / 4
		running: line.activity !== Activity.idle
	}

	Row {
		anchors { left: parent.left; right: trailing.left; rightMargin: line.itemSpacing; verticalCenter: parent.verticalCenter }
		spacing: line.itemSpacing
		clip: true

		Rectangle {
			id: dot
			anchors.verticalCenter: parent.verticalCenter
			visible: line.working
			width: Theme.statusDotSize
			height: Theme.statusDotSize
			radius: width / 2
			color: Theme.accent

			Pulse { target: dot; running: line.working; dimmed: 0.5 }
		}
		Icon {
			anchors.verticalCenter: parent.verticalCenter
			visible: line.waiting || line.retrying
			name: line.activity === Activity.waitingQuestion ? "question" : line.waiting ? "lock" : "retry"
			color: Theme.warning
		}
		ShimmerText {
			anchors.verticalCenter: parent.verticalCenter
			visible: line.working
			label: line.activityText
			color: line.activity === Activity.compacting ? Theme.accent : Theme.textMuted
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
			visible: line.activity !== Activity.idle && line.busyMs >= line.shownFromMs
			text: "\u00b7 " + Format.elapsed(line.busyMs)
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}

	// The right end: the to-do list and the context meter, as in the terminal UI and the web.
	Row {
		id: trailing
		anchors { right: parent.right; verticalCenter: parent.verticalCenter }
		spacing: line.itemSpacing * 2

		TodoProgress {
			anchors.verticalCenter: parent.verticalCenter
			session: line.session
			maxWidth: line.width / 2
			bounds: line
			onDismissed: line.todoDismissed()
		}

		Row {
			id: context
			anchors.verticalCenter: parent.verticalCenter
			spacing: line.itemSpacing
			visible: line.contextMax > 0

			Text {
				textFormat: Text.PlainText
				anchors.verticalCenter: parent.verticalCenter
				text: Format.tokens(line.contextUsed) + " / " + Format.tokens(line.contextMax)
				color: line.contextLevel === StatusLine.ContextLevel.Normal ? Theme.textMuted : line.contextColor
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
}
