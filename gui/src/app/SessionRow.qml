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

	property double now: Date.now()
	property bool renaming: false
	readonly property bool working: activity !== "idle"
	readonly property bool failed: hasError && !working
	readonly property bool retrying: activity === "retrying"
	readonly property bool attention: hasPermission || failed
	readonly property bool hovered: hover.hovered

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

	Timer {
		interval: 1000
		repeat: true
		running: row.busySince > 0 && row.visible
		onTriggered: row.now = Date.now()
	}

	Rectangle {
		id: face
		anchors { fill: parent; leftMargin: Theme.sidebarPadding; rightMargin: Theme.sidebarPadding }
		radius: Theme.radius
		color: row.hasPermission ? Theme.warningSurface
			: row.isCurrent ? Theme.userBubble
			: hover.hovered ? Theme.surfaceHover : "transparent"
		border.color: row.hasPermission ? Theme.warningBorder : row.isCurrent ? Theme.userBubbleBorder : "transparent"
		scale: tap.pressed ? 0.98 : 1

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
			id: title
			x: 30
			y: 8
			width: tail.x - x - 8
			height: 20
			verticalAlignment: Text.AlignVCenter
			text: row.displayTitle
			color: Theme.text
			font.pixelSize: Theme.fontSize - 2
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
			font.pixelSize: Theme.fontSize - 2
			clip: true
			onAccepted: row.commitRename()
			onActiveFocusChanged: if (!activeFocus) row.commitRename()
			Keys.onEscapePressed: row.renaming = false
		}

		Text {
			x: 30
			y: 30
			width: parent.width - x - 12
			text: row.hasPermission ? qsTr("Needs permission")
				: row.failed ? row.lastError.split("\n")[0]
				: row.retrying ? qsTr("Retrying")
				: row.activity === "compacting" ? qsTr("Compacting")
				: row.busySince > 0 ? row.dirName + " · " + Format.elapsed(row.now - row.busySince)
				: row.dirName
			color: row.hasPermission || row.retrying ? Theme.warning : row.failed ? Theme.danger : Theme.textMuted
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
					name: "lock"
					size: 16
					color: Theme.warning
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
					color: Theme.success
					visible: row.todoAllDone && !row.attention
				}
				Text {
					anchors.verticalCenter: parent.verticalCenter
					text: row.todoDone + "/" + row.todoTotal
					color: row.todoAllDone ? Theme.success : Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					visible: row.todoTotal > 0 && !row.attention
				}
			}

			CircleButton {
				anchors.centerIn: parent
				implicitWidth: 22
				implicitHeight: 22
				opacity: hover.hovered ? 1 : 0
				visible: opacity > 0
				Accessible.name: qsTr("Close session")
				onClicked: Workspace.closeSession(row.sessionId)

				Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Icon {
					anchors.centerIn: parent
					name: "close"
					size: 16
					color: Theme.textMuted
				}
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
		onTapped: menu.popup()
	}

	ContextMenu {
		id: menu
		entries: [{text: qsTr("Rename")}, {text: qsTr("Close"), danger: true}]
		onTriggered: index => index === 0 ? row.startRename() : Workspace.closeSession(row.sessionId)
	}
}
