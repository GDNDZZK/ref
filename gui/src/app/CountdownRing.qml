import QtQuick
import QtQuick.Shapes
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

	property real now: Date.now()
	property real total: 1 // ms; the wait when the ring started

	implicitWidth: 16
	implicitHeight: 16

	function start() {
		now = Date.now()
		total = Math.max(1, runAt - now)
	}

	onRunningChanged: if (running) start()
	Component.onCompleted: if (running) start()

	FrameAnimation {
		running: ring.running
		onTriggered: ring.now = Date.now()
	}

	Shape {
		anchors.fill: parent
		preferredRendererType: Shape.CurveRenderer

		ShapePath {
			strokeColor: Theme.border
			strokeWidth: ring.strokeWidth
			fillColor: "transparent"
			PathAngleArc {
				centerX: ring.width / 2
				centerY: ring.height / 2
				radiusX: ring.radius
				radiusY: ring.radius
				startAngle: 0
				sweepAngle: ring.almostFullTurn
			}
		}

		ShapePath {
			strokeColor: ring.color
			strokeWidth: ring.strokeWidth
			fillColor: "transparent"
			capStyle: ShapePath.RoundCap
			PathAngleArc {
				centerX: ring.width / 2
				centerY: ring.height / 2
				radiusX: ring.radius
				radiusY: ring.radius
				startAngle: -90 // twelve o'clock
				sweepAngle: ring.sweep
			}
		}
	}
}
