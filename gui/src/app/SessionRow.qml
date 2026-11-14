import QtQuick
import RefGui

// A running session in the sidebar: state dot, title, directory and run time, to-do progress.
// Permission requests and failed turns stand out.
Item {
	id: row

	required property string sessionId
	required property string displayTitle
	required property string name
	required property string dirName
	required property string activity
	required property string lastError
	required property bool hasError
	required property bool hasPermission
	required property double busySince
	required property int todoDone
	required property int todoTotal
	required property bool todoAllDone
	required property bool isCurrent
	required property bool isTile

	property bool renaming: false
	readonly property bool working: Activity.isWorking(activity)
	readonly property bool failed: Activity.isFailed(activity, hasError)
	readonly property bool retrying: activity === Activity.retrying
	readonly property bool attention: hasPermission || failed
	readonly property bool hovered: hover.hovered
	readonly property int renameEntry: 0 // index in the context menu

	// What the line under the title says.
	readonly property string subtitle: {
		if (activity === Activity.waitingQuestion)
			return qsTr("Waiting for your answer")
		if (hasPermission)
			return qsTr("Needs permission")
		if (failed)
			return lastError.split("\n")[0]
		if (retrying)
			return qsTr("Retrying")
		if (activity === Activity.compacting)
			return qsTr("Compacting")
		return busySince > 0 ? dirName + " · " + Format.elapsed(ticker.now - busySince) : dirName
	}
	readonly property color faceColor: {
		if (hasPermission)
			return Theme.warningSurface
		if (isCurrent)
			return Theme.userBubble
		return hover.hovered ? Theme.surfaceHover : "transparent"
	}
	readonly property color borderColor: {
		if (hasPermission)
			return Theme.warningBorder
		return isCurrent ? Theme.userBubbleBorder : "transparent"
	}

	function startRename() {
		renaming = true
		renameInput.text = name !== "" ? name : displayTitle
		renameInput.forceActiveFocus()
		renameInput.selectAll()
	}

	function commitRename() {
		if (!renaming)
			return
		renaming = false
		if (renameInput.text.trim() !== name)
			Workspace.renameSession(sessionId, renameInput.text.trim())
	}

	width: ListView.view ? ListView.view.width : 0
	height: Theme.sidebarRowHeight

	Ticker {
		id: ticker
		interval: Format.msPerSecond
		running: row.busySince > 0 && row.visible
	}

	Rectangle {
		anchors { fill: parent; leftMargin: Theme.sidebarPadding; rightMargin: Theme.sidebarPadding }
		radius: Theme.radius
		color: row.faceColor
		border.color: row.borderColor
		scale: tap.pressed ? Motion.pressedScaleRow : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		// Marks the sessions shown as tiles in split view.
		Rectangle {
			width: 2
			height: parent.height - 2 * 10
			anchors.verticalCenter: parent.verticalCenter
			radius: 1
			color: Theme.accent
			opacity: row.isTile ? 1 : 0

			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		SessionDot {
			x: 12
			y: 14
			activity: row.activity
			hasError: row.hasError
		}

		Text {
			textFormat: Text.PlainText
			id: title
			x: 30
			y: 8
			width: tail.x - x - 8
			height: 20
			verticalAlignment: Text.AlignVCenter
			text: row.displayTitle
			color: Theme.text
			font.pixelSize: Theme.compactFontSize
			elide: Text.ElideRight
			visible: !row.renaming
		}

		TextInput {
			id: renameInput
			x: 30
			y: 8
			width: tail.x - x - 8
			height: 20
			visible: row.renaming
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.compactFontSize
			clip: true
			onAccepted: row.commitRename()
			onActiveFocusChanged: if (!activeFocus) row.commitRename()
			Keys.onEscapePressed: row.renaming = false
		}

		Text {
			textFormat: Text.PlainText
			x: 30
			y: 30
			width: parent.width - x - 12
			text: row.subtitle
			color: row.hasPermission || row.retrying ? Theme.warningText : row.failed ? Theme.dangerText : Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			elide: Text.ElideRight
		}

		// Right side: the mark that needs attention, else the to-do progress; the close button on hover.
		Item {
			id: tail
			x: parent.width - width - 12
			y: 8
			width: 22
			height: 20

			Row {
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				spacing: 4
				opacity: hover.hovered ? 0 : 1

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: row.activity === Activity.waitingQuestion ? "question" : "lock"
					size: 16
					color: Theme.warningText
					visible: row.hasPermission
				}
				AlertBadge {
					anchors.verticalCenter: parent.verticalCenter
					implicitWidth: 14
					implicitHeight: 14
					visible: row.failed && !row.hasPermission
				}
				Icon {
					anchors.verticalCenter: parent.verticalCenter
					name: "check"
					size: 16
					color: Theme.successText
					visible: row.todoAllDone && !row.attention
				}
				Text {
					textFormat: Text.PlainText
					anchors.verticalCenter: parent.verticalCenter
					text: row.todoDone + "/" + row.todoTotal
					color: row.todoAllDone ? Theme.successText : Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					visible: row.todoTotal > 0 && !row.attention
				}
			}

			IconButton {
				anchors.centerIn: parent
				buttonSize: 22
				iconName: "close"
				iconSize: 16
				opacity: hover.hovered ? 1 : 0
				visible: opacity > 0
				Accessible.name: qsTr("Close session")
				onClicked: Workspace.closeSession(row.sessionId)

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	HoverHandler {
		id: hover
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		onTapped: Workspace.openSession(row.sessionId)
		onDoubleTapped: row.startRename()
	}
	TapHandler {
		acceptedButtons: Qt.RightButton
		onTapped: point => menu.openAt(point.position)
	}

	ContextMenu {
		id: menu
		entries: [{text: qsTr("Rename")}, {text: qsTr("Close"), danger: true}]
		onTriggered: index => {
			if (index === row.renameEntry)
				row.startRename()
			else
				Workspace.closeSession(row.sessionId)
		}
	}
}
