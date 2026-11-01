pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import RefGui

// The session's to-do list on the status line, as in the terminal UI: the item worked on and
// how many are done, or a check mark when all are. A click opens the whole list above it.
Item {
	id: root

	required property ChatSession session
	// The most of the status line this item may take.
	property real maxWidth: 0
	// What the popup stays inside: the status line, which is as wide as the conversation.
	required property Item bounds

	// The keyboard was given back by Esc in the popup.
	signal dismissed()

	readonly property int itemSpacing: 6
	readonly property int markerSize: 8
	readonly property int popupWidth: 360
	readonly property int popupMaxHeight: 320
	readonly property int popupGap: 6
	readonly property int popupPadding: 6
	readonly property int headerIndent: 8
	readonly property int headerPadding: 6
	readonly property bool finished: session.todoCurrent === ""
	readonly property string counts: session.todoDone + "/" + session.todoTotal
	readonly property bool working: session.todoCurrentStatus === TodoStatus.inProgress

	visible: session.todoTotal > 0
	// The natural width of the parts, not of the row: the item's text is cut to what is left.
	readonly property real naturalWidth: finished ? tail.implicitWidth
		: marker.width + current.implicitWidth + tail.implicitWidth + 2 * itemSpacing

	implicitWidth: Math.min(naturalWidth, maxWidth)
	implicitHeight: content.implicitHeight

	// A new session shows its own list.
	onSessionChanged: popup.close()
	onVisibleChanged: if (!visible) popup.close()

	Row {
		id: content
		spacing: root.itemSpacing

		// Working: a pulsing dot; waiting: a ring.
		Rectangle {
			id: marker
			anchors.verticalCenter: parent.verticalCenter
			visible: !root.finished
			width: root.markerSize
			height: root.markerSize
			radius: width / 2
			color: root.working ? Theme.accent : "transparent"
			border.color: root.working ? Theme.accent : Theme.textMuted
			border.width: root.working ? 0 : 1

			Pulse { target: marker; running: root.working && root.visible; dimmed: 0.5 }
		}
		Text {
			id: current
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			visible: !root.finished
			// What is left of the width after the marker and the counts; the counts always show.
			width: Math.max(0, root.implicitWidth - marker.width - tail.implicitWidth - 2 * root.itemSpacing)
			text: root.session.todoCurrent
			elide: Text.ElideRight
			color: root.working ? Theme.text : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
		Text {
			id: tail
			textFormat: Text.PlainText
			anchors.verticalCenter: parent.verticalCenter
			text: root.finished ? "\u2713 " + root.counts : "\u00b7 " + root.counts
			color: root.finished ? Theme.success : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
		}
	}

	HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
	TapHandler { onTapped: popup.visible ? popup.close() : popup.open() }
	Accessible.role: Accessible.Button
	Accessible.name: qsTr("Todo %1").arg(root.counts)
	Accessible.onPressAction: popup.visible ? popup.close() : popup.open()

	Popup {
		id: popup

		// Above the item, its right edge on the item's, and inside `bounds`: the status line is at the bottom of the pane.
		parent: root
		y: -height - root.popupGap
		width: Math.min(root.popupWidth, root.bounds.width)
		height: Math.min(header.height + list.contentHeight + 2 * root.popupPadding, root.popupMaxHeight)
		padding: root.popupPadding
		focus: true
		closePolicy: Popup.CloseOnPressOutside

		onAboutToShow: x = Math.max(root.mapFromItem(root.bounds, 0, 0).x, root.width - width)

		background: Rectangle {
			radius: Theme.radius
			color: Theme.surface
			border.color: Theme.border
		}

		contentItem: Item {
			focus: true

			Keys.onEscapePressed: {
				popup.close()
				root.dismissed()
			}

			Text {
				id: header
				textFormat: Text.PlainText
				x: root.headerIndent
				width: parent.width - 2 * x
				height: implicitHeight + root.headerPadding
				verticalAlignment: Text.AlignVCenter
				text: qsTr("Todo %1").arg(root.counts)
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				font.bold: true
			}

			Flickable {
				id: list
				anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
				contentWidth: width
				contentHeight: todos.height
				interactive: contentHeight > height
				boundsBehavior: Flickable.StopAtBounds
				clip: true

				TodoList {
					id: todos
					width: list.width
					items: root.session.todoItems
				}
			}
		}
	}
}
