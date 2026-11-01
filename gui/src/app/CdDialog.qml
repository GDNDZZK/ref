import QtQuick
import QtQuick.Dialogs
import RefGui

// Changes the working directory of one session.
SessionDialog {
	id: dialog

	property bool gone: false

	function apply() {
		Workspace.setSessionCwd(sessionId, path.text.trim(), (ok, reason) => {
			if (gone)
				return
			if (ok)
				close()
			else
				path.errorText = reason
		})
	}

	title: qsTr("Change directory")
	onAccepted: apply()
	Component.onCompleted: {
		path.text = Workspace.info(sessionId).cwd
		path.select()
	}
	Component.onDestruction: gone = true

	buttons: [
		DialogButton {
			text: qsTr("Browse…")
			visible: Workspace.backendLocal
			onClicked: folders.open()
		},
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Change")
			kind: "primary"
			onClicked: dialog.apply()
		}
	]

	FieldBody {
		PathField {
			id: path
			width: parent.width
			placeholderText: qsTr("Directory")
			onAccepted: dialog.apply()
			onTextChanged: errorText = ""
		}
	}

	FolderDialog {
		id: folders
		onAccepted: path.text = Format.localPath(selectedFolder)
	}
}
