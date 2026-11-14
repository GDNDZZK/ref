pragma Singleton
import QtQuick

// The ids of the commands QML runs by name; the registry is in workspace_commands.cpp.
QtObject {
	readonly property string sidebar: "sidebar"
	readonly property string newSession: "new"
	readonly property string newSessionIn: "new-in"
	readonly property string watch: "watch"
	readonly property string mcp: "mcp"
	readonly property string settings: "settings"
	readonly property string agents: "agents"
	readonly property string models: "models"
}
