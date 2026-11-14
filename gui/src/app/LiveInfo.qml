import QtQuick
import RefGui

// The current facts about one session (see SessionList for the keys), kept up to date as
// the backend reports changes. Empty fields while the session is not known.
QtObject {
	id: info

	property string sessionId: ""
	property var data: Workspace.info(sessionId)

	function refresh() {
		data = Workspace.info(sessionId)
	}

	property Connections updates: Connections {
		target: Workspace.sessions

		function onDataChanged() { info.refresh() }
		function onModelReset() { info.refresh() }
		function onRowsInserted() { info.refresh() }
	}

	onSessionIdChanged: refresh()
}
