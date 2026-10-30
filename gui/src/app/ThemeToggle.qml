pragma ComponentBehavior: Bound
// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Sun / moon toggle. Rays retract and a "bite" slides over the disc to form the moon.
// Switching themes is rare, so this morph may overshoot.
CircleButton {
	id: toggle

	readonly property bool dark: Theme.dark

	Accessible.name: dark ? qsTr("Switch to light theme") : qsTr("Switch to dark theme")

	Item {
		id: glyph
		anchors.centerIn: parent
		width: 16
		height: 16

		Repeater {
			model: 8

			Item {
				required property int index
				anchors.fill: parent
				rotation: index * 45

				Rectangle {
					x: (parent.width - width) / 2
					y: toggle.dark ? 6 : -1
					width: 2
					height: 4
					radius: 1
					color: Theme.text
					opacity: toggle.dark ? 0 : 1

					Behavior on y { NumberAnimation { duration: Motion.slow; easing.type: Motion.emphasisEase } }
					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
			}
		}

		Rectangle {
			anchors.centerIn: parent
			width: toggle.dark ? 14 : 8
			height: width
			radius: width / 2
			color: Theme.text

			Behavior on width { NumberAnimation { duration: Motion.slow; easing.type: Motion.emphasisEase } }
		}

		// Painted in the button's own color, so it reads as a cut-out.
		Rectangle {
			x: toggle.dark ? 6 : 13
			y: toggle.dark ? -2 : -9
			width: 11
			height: 11
			radius: 5.5
			color: toggle.color
			scale: toggle.dark ? 1 : 0

			Behavior on x { NumberAnimation { duration: Motion.slow; easing.type: Motion.emphasisEase } }
			Behavior on y { NumberAnimation { duration: Motion.slow; easing.type: Motion.emphasisEase } }
			Behavior on scale { NumberAnimation { duration: Motion.slow; easing.type: Motion.emphasisEase } }
		}
	}
}
