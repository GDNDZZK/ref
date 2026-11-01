pragma Singleton
import QtQuick

// The kinds of completion ComposerModel::completionKind() reports.
QtObject {
	readonly property string none: "none"
	readonly property string file: "file"
	readonly property string command: "command"
}
