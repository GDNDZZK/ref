pragma Singleton
import QtQuick

// The tools that talk to other sessions, with the icon each shows (see SessionBody and ToolBody).
QtObject {
	readonly property var icons: ({
		"send_message": "chat-send",
		"create_session": "chat-plus",
		"list_sessions": "chat-list",
		"message_status": "chat-send",
		"close_session": "chat-x",
		"read_session": "chat"
	})

	function isSessionTool(toolName) {
		return Fields.has(icons, toolName)
	}

	// The icon of a session tool; the chat icon for any other name.
	function iconOf(toolName) {
		return isSessionTool(toolName) ? icons[toolName] : "chat"
	}
}
