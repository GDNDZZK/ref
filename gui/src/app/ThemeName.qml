pragma Singleton
import QtQuick

// The values of the theme setting.
QtObject {
	readonly property string system: "system" // follows the system's color scheme
	readonly property string light: "light"
	readonly property string dark: "dark"
}
