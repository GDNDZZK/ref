import QtQuick
import QtQuick.Controls
import RefGui

// Shown when the backend speaks an older protocol than this app: offers to restart it. The
// bar slides in and out; `shownHeight` is the room it takes now, for the content below it.
Rectangle {
	id: banner

	readonly property bool shown: Workspace.protocolOutdated
	readonly property real shownHeight: shown ? Theme.bannerHeight : 0

	width: parent.width
	height: Theme.bannerHeight
	y: shown ? 0 : -height
	opacity: shown ? 1 : 0
	visible: opacity > 0
	color: Theme.warningSurface

	Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Rectangle {
		anchors.bottom: parent.bottom
		width: parent.width
		height: 1
		color: Theme.warningBorder
	}

	Icon {
		x: 18
		anchors.verticalCenter: parent.verticalCenter
		name: "warning"
		size: 16
		color: Theme.warning
	}

	Text {
		anchors { left: parent.left; leftMargin: 44; right: restart.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
		text: qsTr("The backend is older than this app (protocol %1, needs %2).").arg(Workspace.protocolVersion).arg(Workspace.requiredProtocol)
		color: Theme.text
		font.pixelSize: Theme.smallFontSize + 1
		elide: Text.ElideRight
	}

	DialogButton {
		id: restart
		anchors { right: parent.right; rightMargin: 18; verticalCenter: parent.verticalCenter }
		small: true
		text: qsTr("Restart backend")
		kind: "primary"
		enabled: Workspace.canRestartBackend
		onClicked: Workspace.restartBackend()

		HoverHandler { id: hint }
		ToolTip.visible: hint.hovered && !restart.enabled
		ToolTip.text: qsTr("Close or finish running sessions first")
	}
}
