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
		CancelButton {},
		DialogButton {
			text: qsTr("Delete")
			kind: DialogButton.Danger
			onClicked: dialog.remove()
		}
	]

	DialogBody {
		Text {
			textFormat: Text.PlainText
			width: parent.width
			text: qsTr("Delete \"%1\" from history? This cannot be undone.").arg(Workspace.dialogArgs.title ?? "")
			color: Theme.text
			font.pixelSize: Theme.secondaryFontSize
			wrapMode: Text.Wrap
		}
	}
}
