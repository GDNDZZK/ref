pragma ComponentBehavior: Bound
// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// A ">" stroke centered in its box; rotate it to point elsewhere. Drawn rather than
// typed, so it sits at the geometric center regardless of font metrics.
//
// Drawn with rotated rounded-end Rectangles, not a Shape: a Shape is drawn outside the
// clip of an ancestor when it lies wholly outside it (every renderer), so the chevron
// of a card scrolled out of the chat showed over the tile header.
Item {
	id: chevron

	property color color: Theme.textMuted
	property real strokeWidth: 1.5

	readonly property real spanRatio: 0.3 // of the smaller side, tip to either end
	readonly property real halfSpan: Math.min(width, height) * spanRatio // tip to either end, vertically
	readonly property real depth: halfSpan // tip to the ends, horizontally; equal to halfSpan for a right angle at the tip

	implicitWidth: 12
	implicitHeight: 12

	Repeater {
		model: [-1, 1]

		delegate: Rectangle {
			required property int modelData

			visible: chevron.halfSpan > 0
			width: chevron.halfSpan * Math.SQRT2 + chevron.strokeWidth
			height: chevron.strokeWidth
			radius: height / 2
			x: (chevron.width - width) / 2
			y: chevron.height / 2 + modelData * (chevron.halfSpan / 2) - height / 2
			rotation: -modelData * 45
			antialiasing: true
			color: chevron.color
		}
	}
}
