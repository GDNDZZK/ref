pragma Singleton
import QtQuick

// Text for times and sizes.
QtObject {
	function pad(number) {
		return number < 10 ? "0" + number : String(number)
	}

	// How long a turn has run: "45s", "3m 05s", "1h 02m".
	function duration(ms) {
		const seconds = Math.max(0, Math.floor(ms / 1000))
		if (seconds < 60)
			return seconds + "s"
		const minutes = Math.floor(seconds / 60)
		if (minutes < 60)
			return minutes + "m " + pad(seconds % 60) + "s"
		return Math.floor(minutes / 60) + "h " + pad(minutes % 60) + "m"
	}

	// "just now", "5m ago", "3h ago", "Yesterday", then the date.
	function relativeTime(ms) {
		const now = new Date()
		const then = new Date(ms)
		const seconds = Math.max(0, (now.getTime() - ms) / 1000)
		if (seconds < 60)
			return qsTr("just now")
		if (seconds < 3600)
			return qsTr("%1m ago").arg(Math.floor(seconds / 60))
		if (seconds < 86400 && now.getDate() === then.getDate())
			return qsTr("%1h ago").arg(Math.floor(seconds / 3600))
		const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
		if (then.getFullYear() === yesterday.getFullYear() && then.getMonth() === yesterday.getMonth() && then.getDate() === yesterday.getDate())
			return qsTr("Yesterday")
		return then.getFullYear() === now.getFullYear() ? Qt.formatDate(then, "MMM d") : Qt.formatDate(then, "MMM d, yyyy")
	}

	// A token count: "200k", "1M".
	function tokens(count) {
		if (count >= 1000000)
			return (count / 1000000).toFixed(count % 1000000 === 0 ? 0 : 1) + "M"
		if (count >= 1000)
			return Math.round(count / 1000) + "k"
		return String(count)
	}

	// The local path of a file: URL.
	function localPath(url) {
		const text = url.toString()
		return /^file:\/\/\/[A-Za-z]:/.test(text) ? decodeURIComponent(text.slice(8)) : decodeURIComponent(text.replace(/^file:\/\//, ""))
	}
}
