import QtQuick
import RefGui

// Asks before a session is deleted from history for good.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? ""

	function remove() {
		Workspace.deleteHistory(sessionId)
		close()
	}

	title: qsTr("Delete session")
	onAccepted: remove()

	buttons: [
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Delete")
			kind: "danger"
			onClicked: dialog.remove()
		}
	]

	Text {
		textFormat: Text.PlainText
		x: 22
		width: parent.width - 2 * 22
		topPadding: 4
		bottomPadding: 22
		text: qsTr("Delete \"%1\" from history? This cannot be undone.").arg(Workspace.dialogArgs.title ?? "")
		color: Theme.text
		font.pixelSize: Theme.fontSize - 1
		wrapMode: Text.Wrap
	}
}
