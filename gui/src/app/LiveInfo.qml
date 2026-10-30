import QtQuick
import RefGui

// The current facts about one session (see SessionList for the keys), kept up to date as
// the backend reports changes. Empty fields while the session is not known.
QtObject {
	id: info

	property string sessionId: ""
	property var data: Workspace.info(sessionId)

	property Connections updates: Connections {
	    target: Workspace.sessions

	    function onDataChanged() {
	        info.data = Workspace.info(info.sessionId)
	    }
	    function onModelReset() {
	        info.data = Workspace.info(info.sessionId)
	    }
	    function onRowsInserted() {
	        info.data = Workspace.info(info.sessionId)
	    }
	}

	onSessionIdChanged: data = Workspace.info(sessionId)
}
