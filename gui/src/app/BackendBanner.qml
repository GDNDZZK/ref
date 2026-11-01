import QtQuick
import QtQuick.Controls
import RefGui

// Shown when the backend speaks an older protocol than this app: offers to restart it. The
// bar slides in and out; `shownHeight` is the room it takes now, for the content below it.
Rectangle {
	id: banner

	readonly property bool shown: Workspace.protocolOutdated
	readonly property real shownHeight: shown ? Theme.bannerHeight : 0
	readonly property int edgeInset: 18
	readonly property int iconSize: 16
	readonly property int iconGap: 10
	readonly property int textInset: edgeInset + iconSize + iconGap

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
		x: banner.edgeInset
		anchors.verticalCenter: parent.verticalCenter
		name: "warning"
		size: banner.iconSize
		color: Theme.warningText
	}

	Text {
		textFormat: Text.PlainText
		anchors { left: parent.left; leftMargin: banner.textInset; right: restart.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
		text: qsTr("The backend is older than this app (protocol %1, needs %2).").arg(Workspace.protocolVersion).arg(Workspace.requiredProtocol)
		color: Theme.text
		font.pixelSize: Theme.captionFontSize
		elide: Text.ElideRight
	}

	DialogButton {
		id: restart
		anchors { right: parent.right; rightMargin: banner.edgeInset; verticalCenter: parent.verticalCenter }
		small: true
		text: qsTr("Restart backend")
		kind: DialogButton.Primary
		enabled: Workspace.canRestartBackend
		onClicked: Workspace.restartBackend()

		HoverHandler { id: hint }

		PlainToolTip {
			visible: hint.hovered && !restart.enabled
			text: qsTr("Close or finish running sessions first")
		}
	}
}
