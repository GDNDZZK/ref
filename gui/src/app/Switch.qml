import QtQuick
import RefGui

// Toggle switch with keyboard focus and mount-safe thumb animation.
FocusScope {
	id: control

	readonly property int trackWidth: 36
	readonly property int trackHeight: 20
	readonly property int knobInset: 2

	property bool checked: false
	property string accessibleName: ""
	property bool ready: false

	signal toggled(bool value)

	function toggle() {
		if (!enabled)
			return
		toggled(!checked)
	}

	implicitWidth: trackWidth
	implicitHeight: trackHeight
	opacity: enabled ? 1 : Theme.disabledOpacity
	Component.onCompleted: ready = true

	Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

	Keys.onSpacePressed: event => {
		toggle()
		event.accepted = true
	}
	Keys.onReturnPressed: event => {
		toggle()
		event.accepted = true
	}
	Keys.onEnterPressed: event => {
		toggle()
		event.accepted = true
	}

	Rectangle {
		id: track
		anchors.fill: parent
		radius: height / 2
		color: control.checked ? Theme.accent : (hover.hovered && control.enabled ? Theme.surfaceHover : Theme.border)
		border.width: control.activeFocus ? 1 : (!control.checked ? 1 : 0)
		border.color: control.activeFocus ? Theme.accentText : Theme.border
		scale: tap.pressed ? Motion.pressedScaleButton : 1

		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Rectangle {
			width: parent.height - 2 * control.knobInset
			height: width
			radius: width / 2
			y: control.knobInset
			x: control.checked ? parent.width - width - control.knobInset : control.knobInset
			color: control.checked ? Theme.onAccent : Theme.surface

			Behavior on x {
				enabled: control.ready
				NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
			}
			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}
	}

	HoverHandler {
		id: hover
		enabled: control.enabled
		cursorShape: Qt.PointingHandCursor
	}
	TapHandler {
		id: tap
		enabled: control.enabled
		onTapped: {
			control.forceActiveFocus()
			control.toggle()
		}
	}

	Accessible.role: Accessible.CheckBox
	Accessible.name: accessibleName
	Accessible.checked: checked
	Accessible.onToggleAction: toggle()
}
