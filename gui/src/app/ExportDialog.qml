import QtQuick
import RefGui

// Writes the conversation as markdown to a path on the backend's machine.
SessionDialog {
	id: dialog

	function exportNow() {
		Workspace.exportTranscript(sessionId, pathField.text.trim(), replyWhileOpen((ok, result) => {
			if (!ok) {
				pathField.errorText = result
				return
			}
			Workspace.showToast(ToastLevel.info, qsTr("Exported to %1").arg(result))
			close()
		}))
	}

	title: qsTr("Export transcript")
	onAccepted: exportNow()

	buttons: [
		CancelButton {},
		DialogButton {
			text: qsTr("Export")
			kind: DialogButton.Primary
			onClicked: dialog.exportNow()
		}
	]

	DialogBody {
		spacing: 12

		Text {
			textFormat: Text.PlainText
			width: parent.width
			text: qsTr("Write the conversation as markdown to this path on the backend's machine.")
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			wrapMode: Text.Wrap
		}
		PathField {
			id: pathField
			width: parent.width
			cwd: Workspace.info(dialog.sessionId).cwd ?? Workspace.effectiveCwd
			includeFiles: true
			text: "ref-session-" + dialog.sessionId + ".md"
			placeholderText: qsTr("Path")
			onAccepted: dialog.exportNow()
			onTextChanged: errorText = ""
			Component.onCompleted: select()
		}
	}
}
