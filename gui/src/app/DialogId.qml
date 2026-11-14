pragma Singleton
import QtQuick

// The ids of the dialogs (Workspace::openDialog) and the one argument QML passes by name.
QtObject {
	readonly property string palette: "palette"
	readonly property string settings: "settings"
	readonly property string sessions: "sessions"
	readonly property string models: "models"
	readonly property string agents: "agents"
	readonly property string variants: "variants"
	readonly property string help: "help"
	readonly property string mcp: "mcp"
	readonly property string peers: "peers"
	readonly property string exportSession: "export"
	readonly property string cd: "cd"
	readonly property string newIn: "new-in"
	readonly property string closeSession: "close"
	readonly property string rename: "rename"
	readonly property string deleteHistory: "delete-history"

	readonly property string closeMode: "close" // `mode` argument of the sessions dialog: a click closes a live session
}
