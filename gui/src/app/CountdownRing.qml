import QtQuick
import RefGui

// A ring that empties while a delayed tool waits for its start: from the full circle
// to nothing, the free end of the arc moving anticlockwise from the top, at a constant
// speed. The arc stands for the time left when the ring first saw the wait.
Item {
	id: ring

	required property real runAt // epoch ms at which the wait ends
	property bool running: false
	property color color: Theme.warning

	readonly property real remaining: Math.max(0, runAt - now)
	readonly property real strokeWidth: 2
	readonly property real fullTurn: 360
	readonly property real almostFullTurn: 359.99 // an arc of exactly a turn is not drawn
	readonly property real sweep: Math.min(almostFullTurn, remaining / total * fullTurn)
	readonly property real radius: (Math.min(width, height) - strokeWidth) / 2
	readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)

	property real now: Date.now()
	property real total: 1 // ms; the wait when the ring started

	implicitWidth: 16
	implicitHeight: 16

	function start() {
		now = Date.now()
		total = Math.max(1, runAt - now)
	}

	// The row may report the state before the time it runs at.
	onRunningChanged: if (running) start()
	onRunAtChanged: if (running) start()
	Component.onCompleted: if (running) start()

	FrameAnimation {
		running: ring.running
		onTriggered: ring.now = Date.now()
	}

	Canvas {
		id: canvas

		readonly property real degrees: Math.PI / 180
		readonly property real twelveOClock: -Math.PI / 2

		width: Math.max(1, Math.ceil(ring.width * ring.pixelScale))
		height: Math.max(1, Math.ceil(ring.height * ring.pixelScale))
		transformOrigin: Item.TopLeft
		scale: 1 / ring.pixelScale
		renderTarget: Canvas.Image

		function arc(ctx, color, cap, from, sweepDegrees) {
			ctx.strokeStyle = color
			ctx.lineWidth = ring.strokeWidth
			ctx.lineCap = cap
			ctx.beginPath()
			ctx.arc(ring.width / 2, ring.height / 2, ring.radius, from, from + sweepDegrees * degrees, false)
			ctx.stroke()
		}

		onWidthChanged: canvas.requestPaint()
		onHeightChanged: canvas.requestPaint()

		onPaint: {
			const ctx = getContext("2d")
			ctx.setTransform(1, 0, 0, 1, 0, 0)
			ctx.clearRect(0, 0, width, height)
			ctx.setTransform(ring.pixelScale, 0, 0, ring.pixelScale, 0, 0)
			arc(ctx, Theme.border, "butt", 0, ring.almostFullTurn)
			arc(ctx, ring.color, "round", twelveOClock, ring.sweep)
		}

		Connections {
			target: ring
			function onSweepChanged() { canvas.requestPaint() }
			function onColorChanged() { canvas.requestPaint() }
			function onPixelScaleChanged() { canvas.requestPaint() }
		}

		Connections {
			target: Theme
			function onBorderChanged() { canvas.requestPaint() }
		}
	}
}
