import QtQuick
import RefGui

// Asks before closing a session that is still working.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId

	function closeSession() {
		Workspace.closeSession(sessionId)
		close()
	}

	title: qsTr("Close session")
	onAccepted: closeSession()

	buttons: [
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Close")
			kind: "danger"
			onClicked: dialog.closeSession()
		}
	]

	Text {
		textFormat: Text.PlainText
		x: 22
		width: parent.width - 2 * 22
		topPadding: 4
		bottomPadding: 22
		text: qsTr("This session is still working. Closing it stops the work; the conversation stays in history.")
		color: Theme.text
		font.pixelSize: Theme.fontSize - 1
		wrapMode: Text.Wrap
	}
}
