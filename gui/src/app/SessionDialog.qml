import QtQuick
import RefGui

// A dialog about one session: the one the opener named, else the current one.
ModalDialog {
	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	readonly property var session: Workspace.info(sessionId)
}
