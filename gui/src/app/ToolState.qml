pragma Singleton
import QtQuick

// The states of a tool call (ToolBlock.toolState).
QtObject {
	readonly property string streaming: "streaming" // the arguments are still arriving
	readonly property string awaitingPermission: "awaiting_permission"
	readonly property string delayed: "delayed" // waits for its start
	readonly property string running: "running"
	readonly property string done: "done"
	readonly property string error: "error"
	readonly property string denied: "denied"
	readonly property string cancelled: "cancelled"

	// On its way to a result.
	function isActive(state) {
		return state === streaming || state === running || state === delayed
	}
}
