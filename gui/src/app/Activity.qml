pragma Singleton
import QtQuick

// What a session is doing (ChatSession::activity), and what that means for how it is shown.
QtObject {
	readonly property string idle: "idle"
	readonly property string thinking: "thinking"
	readonly property string running: "running"
	readonly property string waitingPermission: "waiting_permission"
	readonly property string waitingQuestion: "waiting_question"
	readonly property string retrying: "retrying"
	readonly property string compacting: "compacting"
}
