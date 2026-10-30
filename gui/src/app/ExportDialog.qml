import QtQuick
import RefGui

// Writes the conversation as markdown to a path on the backend's machine.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	property bool gone: false

	function exportNow() {
		Workspace.exportTranscript(sessionId, field.text.trim(), (ok, result) => {
			if (gone)
				return
			if (!ok) {
				field.errorText = result
				return
			}
			Workspace.showToast("info", qsTr("Exported to %1").arg(result))
			close()
		})
	}

	title: qsTr("Export transcript")
	onAccepted: exportNow()
	Component.onDestruction: gone = true

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

	Column {
		x: 22
		width: parent.width - 2 * 22
		topPadding: 4
		bottomPadding: 22
		spacing: 12

		Text {
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
