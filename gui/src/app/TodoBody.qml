pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// The checklist of a todo call. Always shown: the plan is the point of the call.
TodoList {
	id: body

	required property var call

	items: ToolInfo.todos(call.inputObject)
}
