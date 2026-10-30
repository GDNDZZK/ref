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

	// The first of `keys` that holds text in `input`, else "".
	function firstText(input, keys) {
		const key = keys.find(name => typeof input[name] === "string" && input[name] !== "")
		return key === undefined ? "" : input[key]
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

	// Output of read and grep: `[path#TAG]` headers, then rows `N:text`, `*N:text` (a match) and
	// ` N:text` (context), `...` between non-adjacent ones. Returns [{path, tag, rows, notes}] with
	// rows [{number, text, kind}] (kind "line", "match", "context" or "gap") and notes: other lines.
	function listing(output) {
		const groups = []
		let group = null
		const open = (path, tag) => {
			group = { path: path, tag: tag, rows: [], notes: [] }
			groups.push(group)
		}
		for (const line of output.split("\n")) {
			if (line === "")
				continue
			const header = /^\[(.*)#([0-9A-Fa-f]{4})\]$/.exec(line)
			if (header) {
				open(header[1], header[2])
				continue
			}
			if (group === null)
				open("", "")
			const row = /^([ *]?)(\d+):(.*)$/.exec(line)
			if (row)
				group.rows.push({ number: row[2], text: row[3], kind: row[1] === "*" ? "match" : row[1] === " " ? "context" : "line" })
			else if (line === "...")
				group.rows.push({ number: "", text: "\u22ef", kind: "gap" })
			else
				group.notes.push(line)
		}
		return groups
	}

	// A unified diff as rows [{text, kind}], kind "added", "removed", "hunk", "file" or "context".
	function diffRows(diff) {
		const lines = diff.split("\n")
		if (lines.length > 0 && lines[lines.length - 1] === "")
			lines.pop()
		return lines.map(line => {
			let kind = "context"
			if (line.startsWith("+++") || line.startsWith("---"))
				kind = "file"
			else if (line.startsWith("@@"))
				kind = "hunk"
			else if (line.startsWith("+"))
				kind = "added"
			else if (line.startsWith("-"))
				kind = "removed"
			const marked = kind === "added" || kind === "removed"
			return { text: marked ? line.slice(1) : line, sign: marked ? line.charAt(0) : "", kind: kind }
		})
	}

	// "src/" and "parser.rs" of "src/parser.rs"; a trailing slash stays with the directory.
	function splitPath(path) {
		const trimmed = path.endsWith("/") ? path.slice(0, -1) : path
		const cut = trimmed.lastIndexOf("/")
		return { directory: path.slice(0, cut + 1), name: path.slice(cut + 1), isDirectory: path.endsWith("/") }
	}

	function sizeText(bytes) {
		const kib = 1024
		const mib = kib * kib
		if (bytes >= mib)
			return (bytes / mib).toFixed(1) + " MiB"
		if (bytes >= kib)
			return (bytes / kib).toFixed(1) + " KiB"
		return bytes + " B"
	}
}
