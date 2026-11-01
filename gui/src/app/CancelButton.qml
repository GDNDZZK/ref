import QtQuick
import RefGui

// The Cancel button of a dialog: closes it.
DialogButton {
	text: qsTr("Cancel")
	onClicked: Workspace.closeDialog()
}
