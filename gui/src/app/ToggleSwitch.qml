import QtQuick
import RefGui

// On / off switch. The knob grows a little while pressed and never overshoots.
Item {
	id: toggle

	property bool checked: false

	signal toggled(bool checked)

	implicitWidth: 38
	implicitHeight: 22
	opacity: enabled ? 1 : 0.4

	Rectangle {
		anchors.fill: parent
		radius: height / 2
		color: toggle.checked ? Theme.accent : Theme.border

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	Rectangle {
		readonly property int inset: 3
		y: inset
		width: 16 + (tap.pressed ? 3 : 0)
		height: 16
		radius: height / 2
		x: toggle.checked ? toggle.width - width - inset : inset
		color: toggle.checked ? Theme.onAccent : Theme.textMuted

		Behavior on x { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on width { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}

	HoverHandler {
		enabled: toggle.enabled
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		enabled: toggle.enabled
		onTapped: toggle.toggled(!toggle.checked)
	}

	Accessible.role: Accessible.CheckBox
	Accessible.checked: checked
	Accessible.onToggleAction: if (enabled) toggled(!checked)
}
