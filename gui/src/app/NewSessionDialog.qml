import QtQuick
import RefGui

// Starts a session in a directory of the backend's machine. Only for a backend on another
// machine; for a local one the window shows a folder dialog instead.
ModalDialog {
	id: dialog

	function start() {
		Workspace.newSession(path.text.trim(), closeOrShowError(path))
	}

	title: qsTr("New session")
	onAccepted: start()
	Component.onCompleted: {
		path.text = Workspace.cwd
		path.select()
	}

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

	FieldBody {
		PathField {
			id: path
			width: parent.width
			placeholderText: qsTr("Directory")
			onAccepted: dialog.start()
			onTextChanged: errorText = ""
		}
	}
}
