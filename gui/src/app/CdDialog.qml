import QtQuick
import QtQuick.Dialogs
import RefGui

// Changes the working directory of one session.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
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
			onAccepted: dialog.apply()
			onTextChanged: errorText = ""
		}
	}

	FolderDialog {
		id: folders
		onAccepted: path.text = Format.localPath(selectedFolder)
	}
}
