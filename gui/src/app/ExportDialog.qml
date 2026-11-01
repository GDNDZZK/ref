import QtQuick
import RefGui

// Writes the conversation as markdown to a path on the backend's machine.
SessionDialog {
	id: dialog

	function exportNow() {
		Workspace.exportTranscript(sessionId, field.text.trim(), replyWhileOpen((ok, result) => {
			if (!ok) {
				field.errorText = result
				return
			}
			Workspace.showToast(ToastLevel.info, qsTr("Exported to %1").arg(result))
			close()
		}))
	}

	title: qsTr("Export transcript")
	onAccepted: exportNow()

	buttons: [
		DialogButton {
			text: qsTr("Cancel")
			onClicked: dialog.close()
		},
		DialogButton {
			text: qsTr("Export")
			kind: "primary"
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
		DialogField {
			id: field
			width: parent.width
			text: "ref-session-" + dialog.sessionId + ".md"
			placeholderText: qsTr("Path")
			onAccepted: dialog.exportNow()
			onEdited: errorText = ""
			Component.onCompleted: selectAll()
		}
	}
}
