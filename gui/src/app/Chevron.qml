// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// A ">" stroke centered in its box; rotate it to point elsewhere. Drawn rather than
// typed, so it sits at the geometric center regardless of font metrics.
//
// Drawn on a Canvas, not a Shape: a Shape is drawn outside the clip of an ancestor when it lies
// wholly outside it (every renderer), so the chevron of a card scrolled out of the chat showed
// over the tile header.
Item {
	id: chevron

	property color color: Theme.textMuted
	property real strokeWidth: 1.5

	readonly property real halfSpan: Math.min(width, height) * 0.3 // tip to either end, vertically
	readonly property real depth: halfSpan // tip to the ends, horizontally; equal to halfSpan for a right angle at the tip
	readonly property real endsX: (width - depth) / 2

	implicitWidth: 12
	implicitHeight: 12

	Canvas {
		id: canvas

		anchors.fill: parent
		renderTarget: Canvas.Image

		onPaint: {
			const ctx = getContext("2d")
			ctx.reset()
			ctx.strokeStyle = chevron.color
			ctx.lineWidth = chevron.strokeWidth
			ctx.lineCap = "round"
			ctx.lineJoin = "round"
			ctx.beginPath()
			ctx.moveTo(chevron.endsX, chevron.height / 2 - chevron.halfSpan)
			ctx.lineTo(chevron.endsX + chevron.depth, chevron.height / 2)
			ctx.lineTo(chevron.endsX, chevron.height / 2 + chevron.halfSpan)
			ctx.stroke()
		}
		Connections {
			target: chevron
			function onColorChanged() { canvas.requestPaint() }
			function onStrokeWidthChanged() { canvas.requestPaint() }
		}
	}
}
