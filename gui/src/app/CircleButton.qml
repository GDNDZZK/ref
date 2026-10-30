// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Round surface button; the glyph is its child. Set Accessible.name on each use.
//
// Only the face shrinks on press. The handlers sit on the unscaled root: a handler
// tests its parent's bounds through the parent's scale, so on a shrunken root a
// release near the edge would land outside and never count as a tap.
Item {
	id: button

	signal clicked()

	readonly property color color: face.color // for glyphs painted in the button's color
	default property alias content: face.data

	implicitWidth: 34
	implicitHeight: 34

	Rectangle {
		id: face
		anchors.fill: parent
		radius: width / 2
		color: hover.hovered ? Theme.surfaceHover : Theme.surface
		border.color: Theme.border
		scale: tap.pressed ? 0.92 : 1

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	HoverHandler {
		id: hover
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.onPressAction: clicked()
}
