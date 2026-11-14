// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// Pulsing dot that trails the end of streaming text. It chases the end rather than
// animating to it with a Behavior: the end moves every frame while text streams in.
Rectangle {
	id: caret

	property rect anchorRect // cursor rectangle of the last character

	readonly property real targetX: anchorRect.x + 3
	readonly property real targetY: anchorRect.y + (anchorRect.height - height) / 2

	width: Theme.statusDotSize
	height: Theme.statusDotSize
	radius: width / 2
	color: Theme.accent

	Component.onCompleted: {
		x = targetX
		y = targetY
	}

	FrameAnimation {
		running: caret.visible && !(Motion.arrived(caret.x, caret.targetX) && Motion.arrived(caret.y, caret.targetY))
		onTriggered: {
			caret.x = Motion.chase(caret.x, caret.targetX, frameTime)
			caret.y = Motion.chase(caret.y, caret.targetY, frameTime)
		}
	}

	Pulse {
		target: caret
		running: caret.visible
	}
}
