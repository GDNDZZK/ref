pragma Singleton
import QtQuick
import RefGui

// The status of an MCP server as the backend reports it, and the dot colour for it.
QtObject {
	readonly property string connected: "connected"
	readonly property string failed: "failed"
	readonly property string connecting: "connecting"

	function colorOf(status) {
		switch (status) {
		case connected:
			return Theme.success
		case failed:
			return Theme.danger
		case connecting:
			return Theme.accent
		default:
			return Theme.idleDot
		}
	}
}
