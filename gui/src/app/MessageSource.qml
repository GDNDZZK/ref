pragma Singleton
import QtQuick

// Where a user-role message came from (UserBlock.source, MetaBlock.source).
QtObject {
	readonly property string compaction: "compaction" // the summary that replaces the history
	readonly property string human: "human"
	readonly property string peer: "peer" // another session
	readonly property string spawn: "spawn" // the session that created this one
	readonly property string shell: "shell"
	readonly property string reminder: "reminder"
}
