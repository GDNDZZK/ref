import QtQuick

// A clock for texts that count time. `now` is the time of the last tick, in ms since the epoch; it moves only while `running`.
Timer {
	property real now: Date.now()

	repeat: true
	onTriggered: now = Date.now()
}
