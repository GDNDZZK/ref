pragma Singleton
import QtQuick

// Global UI zoom factor driven by Main.qml from Workspace.settings.uiScale / Workspace.startupUiScale,
// so popups and icons in isolated QML tests can read it without Workspace.
QtObject {
	property real factor: 1.0
}
