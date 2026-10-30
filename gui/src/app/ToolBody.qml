import QtQuick
import RefGui

// What a tool card shows when it is open, laid out by the tool. A tool whose result lacks
// what its layout needs falls back to the plain input and output.
Item {
	id: body

	required property var call

	signal showAllRequested()

	readonly property var meta: call.meta
	readonly property string kind: {
		switch (call.toolName) {
		case "edit":
		case "write":
			return typeof meta.diff === "string" && meta.diff !== "" ? "diff" : "generic"
		case "bash":
		case "grep":
		case "find":
		case "webfetch":
		case "todo":
			return call.toolName
		case "read":
			return call.output !== "" && !call.failed && call.toolState === "done" ? "read" : "generic"
		case "send_message":
		case "create_session":
		case "read_session":
		case "list_sessions":
		case "message_status":
		case "close_session":
			return "session"
		default:
			return "generic"
		}
	}

	width: parent ? parent.width : 0
	height: loader.item ? loader.item.height : 0

	Component { id: generic; GenericBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: diff; DiffBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: bash; BashBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: read; ReadBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: grep; GrepBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: find; FindBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: webfetch; WebfetchBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: session; SessionBody { call: body.call; onShowAllRequested: body.showAllRequested() } }
	Component { id: todo; TodoBody { call: body.call } }

	Loader {
		id: loader
		width: parent.width
		sourceComponent: {
			switch (body.kind) {
			case "diff": return diff
			case "bash": return bash
			case "read": return read
			case "grep": return grep
			case "find": return find
			case "webfetch": return webfetch
			case "session": return session
			case "todo": return todo
			default: return generic
			}
		}
	}
}
