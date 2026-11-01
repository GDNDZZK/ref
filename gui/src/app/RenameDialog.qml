import QtQuick
import RefGui

// Gives a session a name. Names are unique among live sessions; the backend says if one is taken.
SessionDialog {
	id: dialog

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
		implicitHeight: field.implicitHeight + 2 * Theme.dialogPadding
		height: implicitHeight

		DialogField {
			id: field
			x: Theme.dialogPadding
			y: Theme.dialogPadding
			width: parent.width - 2 * Theme.dialogPadding
			text: Workspace.info(dialog.sessionId).name
			placeholderText: qsTr("Name")
			onAccepted: dialog.rename()
			onEdited: errorText = ""
			Component.onCompleted: selectAll()
		}
	}
}
