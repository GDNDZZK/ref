import QtQuick
import RefGui

// Asks before closing a session that is still working.
SessionDialog {
	id: dialog

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
		x: Theme.dialogPadding
		width: parent.width - 2 * Theme.dialogPadding
		topPadding: 4
		bottomPadding: Theme.dialogPadding
		text: qsTr("This session is still working. Closing it stops the work; the conversation stays in history.")
		color: Theme.text
		font.pixelSize: Theme.secondaryFontSize
		wrapMode: Text.Wrap
	}
}
