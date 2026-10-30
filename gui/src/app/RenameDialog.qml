import QtQuick
import RefGui

// Gives a session a name. Names are unique among live sessions; the backend says if one is taken.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	property bool gone: false

	function rename() {
		Workspace.renameSession(sessionId, field.text.trim(), (ok, reason) => {
			if (gone)
				return
			if (ok)
				close()
			else
				field.errorText = reason
		})
	}

	title: qsTr("Rename session")
	onAccepted: rename()
	Component.onDestruction: gone = true

	buttons: [
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Rename")
			kind: "primary"
			onClicked: dialog.rename()
		}
	]

	Item {
		width: parent.width
		implicitHeight: field.implicitHeight + 2 * 22
		height: implicitHeight

		DialogField {
			id: field
			x: 22
			y: 22
			width: parent.width - 2 * 22
			text: Workspace.info(dialog.sessionId).name
			placeholderText: qsTr("Name")
			onAccepted: dialog.rename()
			onEdited: errorText = ""
			Component.onCompleted: selectAll()
		}
	}
}
