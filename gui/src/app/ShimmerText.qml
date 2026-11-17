import QtQuick
import QtQuick.Effects
import RefGui

// Text with a highlight band that sweeps across the glyphs on the GPU while work continues.
Text {
	id: shimmer

	property string label
	property color highlight: Theme.text
	property bool running: true

	readonly property int shimmerPeriod: 1600 // ms for one sweep
	readonly property real bandRatio: 0.45
	readonly property real bandWidth: Math.max(24, width * bandRatio)
	property real phase: 0

	textFormat: Text.PlainText
	text: label

	FrameAnimation {
		running: shimmer.running && shimmer.visible && shimmer.width > 0
		onTriggered: shimmer.phase = (shimmer.phase + frameTime * Format.msPerSecond / shimmer.shimmerPeriod) % 1
	}

	Item {
		id: maskItem
		anchors.fill: parent
		layer.enabled: shimmer.running && shimmer.visible && shimmer.width > 0
		visible: false

		Rectangle {
			x: -shimmer.bandWidth + shimmer.phase * (shimmer.width + 2 * shimmer.bandWidth)
			width: shimmer.bandWidth
			height: parent.height
			gradient: Gradient {
				orientation: Gradient.Horizontal
				GradientStop { position: 0.0; color: "transparent" }
				GradientStop { position: 0.5; color: "white" }
				GradientStop { position: 1.0; color: "transparent" }
			}
		}
	}

	Text {
		id: brightText
		anchors.fill: parent
		textFormat: Text.PlainText
		text: shimmer.label
		font: shimmer.font
		color: shimmer.highlight
		visible: false
	}

	MultiEffect {
		anchors.fill: parent
		visible: shimmer.running && shimmer.width > 0
		source: brightText
		maskEnabled: true
		maskSource: maskItem
	}
}
