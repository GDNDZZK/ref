pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Window controls for frameless windows (minimize, maximize/restore, close).
Row {
	id: controls

	required property bool maximized

	signal minimizeRequested()
	signal maximizeRequested()
	signal closeRequested()

	spacing: Theme.windowButtonGap

	IconButton {
		id: minimizeButton
		iconName: "minus"
		Accessible.name: qsTr("Minimize")
		onClicked: controls.minimizeRequested()
	}

	IconButton {
		id: maximizeButton
		iconName: controls.maximized ? "restore" : "maximize"
		Accessible.name: controls.maximized ? qsTr("Restore") : qsTr("Maximize")
		onClicked: controls.maximizeRequested()
	}

	IconButton {
		id: closeButton
		iconName: "close"
		Accessible.name: qsTr("Close")
		onClicked: controls.closeRequested()
	}
}
