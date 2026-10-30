pragma Singleton
import QtQuick

// What a tool call's name and input say about it, for the card's header and body.
QtObject {
	readonly property var icons: ({
		"read": "file-text",
		"edit": "pencil",
		"write": "file-plus",
		"delete": "trash",
		"bash": "terminal",
		"grep": "search",
		"find": "folder-search",
		"webfetch": "globe",
		"todo": "todo",
		"create_session": "chat-plus",
		"send_message": "chat-send",
		"message_status": "chat-send",
		"list_sessions": "chat-list",
		"close_session": "chat-x",
		"read_session": "chat"
	})
	readonly property string mcpPrefix: "mcp"

	// The tool's icon; MCP tools are named after their server.
	function icon(name) {
		const known = icons[name]
		if (known)
			return known
		return name.startsWith(mcpPrefix + "_") || name.startsWith(mcpPrefix + "__") ? "mcp" : "tool"
	}

	// The arguments as an object; `{}` for text that is not JSON (yet).
	function parse(inputJson) {
		if (inputJson === "")
			return {}
		try {
			const value = JSON.parse(inputJson)
			return value !== null && typeof value === "object" ? value : {}
		} catch (error) {
			return {}
		}
	}

	// The todo items of a `todo` call: [{content, status}], bad entries skipped.
	function todos(input) {
		if (!Array.isArray(input.todos))
			return []
		return input.todos
			.filter(item => item && typeof item.content === "string" && item.content.trim() !== "")
			.map(item => ({ content: item.content.trim(), status: typeof item.status === "string" ? item.status : "pending" }))
	}

	function firstLine(text) {
		const end = text.indexOf("\n")
		return end < 0 ? text : text.slice(0, end)
	}

	// One line from the input that tells calls of the same tool apart.
	function detail(name, input) {
		const text = key => typeof input[key] === "string" ? input[key] : ""
		switch (name) {
		case "bash":
			return text("command") === "" ? "" : "$ " + firstLine(text("command"))
		case "read":
		case "write":
		case "delete":
			return text("path")
		case "edit": {
			const edits = Array.isArray(input.edits) ? input.edits.length : 0
			return edits > 1 ? qsTr("%1 (%2 edits)").arg(text("path")).arg(edits) : text("path")
		}
		case "grep":
			return text("path") === "" ? "\"" + text("pattern") + "\"" : qsTr("\"%1\" in %2").arg(text("pattern")).arg(text("path"))
		case "find":
			return text("pattern") !== "" ? text("pattern") : text("path")
		case "webfetch":
			return text("url")
		case "send_message":
			return text("to") === "" ? "" : "\u2192 " + text("to")
		case "create_session":
			return text("name")
		case "read_session":
		case "close_session":
			return text("session")
		case "todo": {
			const items = todos(input)
			const done = items.filter(item => item.status === "completed").length
			return items.length === 0 ? "" : qsTr("%1/%2 done").arg(done).arg(items.length)
		}
		default:
			return ""
		}
	}
}
