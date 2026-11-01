import QtQuick
import RefGui

// A CopyButton that shows while the pointer is over `hoverTarget` (the item it sits on).
CopyButton {
	id: button

	property Item hoverTarget: parent

	opacity: hover.hovered ? 1 : 0

	Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

	HoverHandler {
		id: hover
		parent: button.hoverTarget
	}
}
