import QtQuick
import QtQuick.Controls
import RefGui

// Vertical scroll bar in the theme's colors; the handle shows while scrolling.
ScrollBar {
	id: bar

	policy: ScrollBar.AsNeeded

	contentItem: ScrollHandle {
		implicitWidth: thickness
		pressed: bar.pressed
		opacity: (bar.active || bar.hovered) && bar.size < 1 ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
}
