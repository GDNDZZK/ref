import QtQuick
import RefGui

// Starts a session in a directory of the backend's machine. Only for a backend on another
// machine; for a local one the window shows a folder dialog instead.
ModalDialog {
	id: dialog

	property bool gone: false

	function start() {
		Workspace.newSession(path.text.trim(), (ok, reason) => {
			if (gone)
				return
			if (ok)
				close()
			else
				path.errorText = reason
		})
	}

	title: qsTr("New session")
	onAccepted: start()
	Component.onCompleted: {
		path.text = Workspace.cwd
		path.select()
	}
	Component.onDestruction: gone = true

	buttons: [
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Start")
			kind: "primary"
			onClicked: dialog.start()
		}
	]

	Item {
		width: parent.width
		implicitHeight: path.implicitHeight + 2 * 22
		height: implicitHeight

		PathField {
			id: path
			x: 22
			y: 22
			width: parent.width - 2 * 22
			placeholderText: qsTr("Directory")
			onAccepted: dialog.start()
			onTextChanged: errorText = ""
		}
	}
}
