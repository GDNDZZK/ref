import QtQuick
import RefGui

// Gives a session a name. Names are unique among live sessions; the backend says if one is taken.
SessionDialog {
	id: dialog

	function rename() {
		Workspace.renameSession(sessionId, field.text.trim(), closeOrShowError(field))
	}

	title: qsTr("Rename session")
	onAccepted: rename()

	buttons: [
		CancelButton {},
		DialogButton {
			text: qsTr("Rename")
			kind: DialogButton.Primary
			onClicked: dialog.rename()
		}
	]

	FieldBody {
		DialogField {
			id: field
			width: parent.width
			text: Workspace.info(dialog.sessionId).name
			placeholderText: qsTr("Name")
			onAccepted: dialog.rename()
			onEdited: errorText = ""
			Component.onCompleted: selectAll()
		}
	}
}
