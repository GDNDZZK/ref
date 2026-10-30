import QtQuick
import RefGui

// Text with a highlight that sweeps over it, for something that goes on. Each letter takes
// a color between `color` and `highlight` by its distance from the highlight's center.
Text {
	id: shimmer

	property string label
	property color highlight: Theme.text
	property bool running: true

	readonly property int period: 1600 // ms for one sweep
	readonly property real highlightWidth: 3 // letters
	property real phase: 0 // 0..1 along the sweep

	function letterColor(index) {
		const length = label.length
		const center = -highlightWidth + phase * (length + 2 * highlightWidth)
		const nearness = Math.max(0, 1 - Math.abs(index - center) / highlightWidth)
		return String(Qt.tint(color, Qt.alpha(highlight, nearness)))
	}

	function escaped(letter) {
		return letter === "<" ? "&lt;" : letter === "&" ? "&amp;" : letter === " " ? "&nbsp;" : letter
	}

	textFormat: Text.RichText
	text: Array.from(label, (letter, index) => "<span style=\"color:" + letterColor(index) + "\">" + escaped(letter) + "</span>").join("")

	FrameAnimation {
		running: shimmer.running
		onTriggered: shimmer.phase = (shimmer.phase + frameTime * Format.msPerSecond / shimmer.period) % 1
	}
}
