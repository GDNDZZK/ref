pragma Singleton
import QtQuick

// Text for times, durations and token counts, as the terminal front end writes them.
QtObject {
	readonly property int msPerSecond: 1000
	readonly property int secondsPerMinute: 60
	readonly property int secondsPerHour: 3600
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
		const pad = n => (n < 10 ? "0" : "") + n
		if (s < secondsPerHour)
			return Math.floor(s / secondsPerMinute) + "m " + pad(s % secondsPerMinute)  + "s"
		return Math.floor(s / secondsPerHour) + "h " + pad(Math.floor(s / secondsPerMinute) % 60) + "m"
	}

	// "950", "12.3k", "1.0M"
	function tokens(n) {
		if (n >= millionRoundsUpFrom)
			return (n / million).toFixed(1) + "M"
		if (n >= thousand)
			return (n / thousand).toFixed(1) + "k"
		return String(n)
	}
}
