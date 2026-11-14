pragma Singleton
import QtQuick

// The statuses of a todo item (the `todo` tool's input).
QtObject {
	readonly property string completed: "completed"
	readonly property string cancelled: "cancelled"
	readonly property string inProgress: "in_progress"
}
