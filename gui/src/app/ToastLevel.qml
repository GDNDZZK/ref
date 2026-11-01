pragma Singleton
import QtQuick

// The levels of a toast (Workspace::showToast).
QtObject {
	readonly property string info: "info"
	readonly property string warn: "warn"
	readonly property string error: "error"
}
