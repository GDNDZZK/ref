pragma Singleton
import QtQuick

// Text for times, durations and token counts, as the terminal front end writes them.
QtObject {
	readonly property int msPerSecond: 1000
	readonly property int secondsPerMinute: 60
	readonly property int secondsPerHour: 3600
	readonly property int secondsPerDay: 86400
	readonly property int minutesPerHour: 60
	readonly property int padBelow: 10 // a number below this gets a leading zero in "1m 05s"
	readonly property int wholeSecondFrom: 10000 // ms; below this a duration shows a decimal
	readonly property int thousand: 1000
	readonly property int million: 1000000
	readonly property int millionRoundsUpFrom: 999950 // 999.95k shows as 1.0M, not 1000.0k

	// "14:02:11"
	function clock(ms) {
		return Qt.formatTime(new Date(ms), "HH:mm:ss")
	}

	// "123ms", "1.2s", "45s", "3m 05s", "1h 02m"
	function duration(ms) {
		const t = Math.max(0, ms)
		if (t < msPerSecond)
			return Math.round(t) + "ms"
		if (t < wholeSecondFrom)
			return (t / msPerSecond).toFixed(1) + "s"
		return elapsed(t)
	}

	// Coarse, for counters that tick: "3s", "1m 05s", "1h 02m"
	function elapsed(ms) {
		const s = Math.floor(Math.max(0, ms) / msPerSecond)
		if (s < secondsPerMinute)
			return s + "s"
		const pad = n => (n < padBelow ? "0" : "") + n
		if (s < secondsPerHour)
			return Math.floor(s / secondsPerMinute) + "m " + pad(s % secondsPerMinute)  + "s"
		return Math.floor(s / secondsPerHour) + "h " + pad(Math.floor(s / secondsPerMinute) % minutesPerHour) + "m"
	}

	// A countdown as elapsed() writes it, rounded up: with 0.2 s left it still reads "1s", and "0s" only once it is over.
	function countdown(ms) {
		return elapsed(Math.max(0, ms) + msPerSecond - 1)
	}

	// "950", "12.3k", "1.0M"
	function tokens(n) {
		if (n >= millionRoundsUpFrom)
			return (n / million).toFixed(1) + "M"
		if (n >= thousand)
			return (n / thousand).toFixed(1) + "k"
		return String(n)
	}

	function isSameDay(a, b) {
		return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
	}

	// "just now", "5m ago", "3h ago", "Yesterday", then the date.
	function relativeTime(ms) {
		const now = new Date()
		const then = new Date(ms)
		const seconds = Math.max(0, (now.getTime() - ms) / msPerSecond)
		if (seconds < secondsPerMinute)
			return qsTr("just now")
		if (seconds < secondsPerHour)
			return qsTr("%1m ago").arg(Math.floor(seconds / secondsPerMinute))
		if (seconds < secondsPerDay && now.getDate() === then.getDate())
			return qsTr("%1h ago").arg(Math.floor(seconds / secondsPerHour))
		const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
		if (isSameDay(then, yesterday))
			return qsTr("Yesterday")
		return then.getFullYear() === now.getFullYear() ? Qt.formatDate(then, "MMM d") : Qt.formatDate(then, "MMM d, yyyy")
	}

	// The local path of a file: URL.
	function localPath(url) {
		const text = url.toString()
		const hasWindowsDrive = /^file:\/\/\/[A-Za-z]:/.test(text)
		if (hasWindowsDrive)
			return decodeURIComponent(text.slice("file:///".length)) // "C:/dir", without the slash before the drive
		return decodeURIComponent(text.replace(/^file:\/\//, ""))
	}
}
