// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import QtQuick.Shapes
import RefGui

// A ">" stroke centered in its box; rotate it to point elsewhere. Drawn rather than
// typed, so it sits at the geometric center regardless of font metrics.
//
// The size lives on a plain Item: a Shape sets its own implicit size from the path's
// bounds, so a path computed from the Shape's size would feed back into itself.
Item {
	id: chevron

	property color color: Theme.textMuted
	property real strokeWidth: 1.5

	readonly property real halfSpan: Math.min(width, height) * 0.3 // tip to either end, vertically
	readonly property real depth: halfSpan // tip to the ends, horizontally; equal to halfSpan for a right angle at the tip
	readonly property real endsX: (width - depth) / 2

	implicitWidth: 12
	implicitHeight: 12

	Shape {
		anchors.fill: parent
		preferredRendererType: Shape.CurveRenderer

		ShapePath {
			strokeColor: chevron.color
			strokeWidth: chevron.strokeWidth
			fillColor: "transparent"
			capStyle: ShapePath.RoundCap
			joinStyle: ShapePath.RoundJoin

			startX: chevron.endsX
			startY: chevron.height / 2 - chevron.halfSpan
			PathLine { x: chevron.endsX + chevron.depth; y: chevron.height / 2 }
			PathLine { x: chevron.endsX; y: chevron.height / 2 + chevron.halfSpan }
		}
	}
}
