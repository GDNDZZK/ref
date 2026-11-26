pragma ComponentBehavior: Bound
// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// Sun / moon toggle. Rays retract and a "bite" slides over the disc to form the moon.
// Hovering anticipates the next theme:
// - Dark mode (about to switch to light): dawn bursts from behind the lower-left rim as the moon
//   tilts and faint stars drift away.
// - Light mode (about to switch to dark): a lunar eclipse shadow creeps in from the upper-right rim
//   while the sun rays contract inward and dim.
CircleButton {
	id: toggle

	readonly property bool dark: Theme.dark

	rimColor: toggle.dark ? Qt.rgba(0.70, 0.82, 1.0, 1.0) : Qt.rgba(0.18, 0.22, 0.34, 1.0)
	rimCoreColor: toggle.dark ? Qt.rgba(1.0, 1.0, 1.0, 1.0) : Qt.rgba(0.08, 0.11, 0.20, 1.0)
	rimBiasAngle: toggle.dark ? Math.PI * 0.78 : -Math.PI * 0.25
	rimStrengthScale: toggle.dark ? 1.35 : 0.95
	eclipseShade: !toggle.dark && toggle.hovered ? 1.0 : 0.0

	Accessible.name: toggle.dark ? qsTr("Switch to light theme") : qsTr("Switch to dark theme")

	Item {
		id: glyph
		anchors.centerIn: parent
		width: 16
		height: 16
		rotation: toggle.hovered ? (toggle.dark ? -12 : -22) : 0
		scale: toggle.hovered ? (toggle.dark ? 1.08 : 0.96) : 1.0

		Behavior on rotation {
			SpringAnimation {
				spring: 4.5
				damping: 0.28
				epsilon: 0.05
			}
		}
		Behavior on scale {
			SpringAnimation {
				spring: 4.8
				damping: 0.30
				epsilon: 0.002
			}
		}

		Repeater {
			model: 8

			Item {
				required property int index
				anchors.fill: parent
				rotation: index * 45

				Rectangle {
					x: (parent.width - width) / 2
					y: toggle.dark ? 6 : (toggle.hovered ? 1.5 : -1)
					width: 2
					height: 4
					radius: 1
					color: Theme.text
					opacity: toggle.dark ? 0 : (toggle.hovered ? 0.52 : 1)

					Behavior on y {
						SpringAnimation {
							spring: 4.6
							damping: 0.32
							epsilon: 0.05
						}
					}
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

		// Subtle dawn-drifting star glints beside the crescent in dark mode on hover.
		Rectangle {
			x: toggle.dark && toggle.hovered ? 12.2 : 10.6
			y: toggle.dark && toggle.hovered ? 0.8 : 2.2
			width: 2.8
			height: 2.8
			radius: 0.6
			rotation: 45
			antialiasing: true
			color: Theme.text
			scale: toggle.dark && toggle.hovered ? 1 : 0
			opacity: toggle.dark && toggle.hovered ? 0.88 : 0

			Behavior on x { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
			Behavior on y { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
			Behavior on scale { SpringAnimation { spring: 4.8; damping: 0.26; epsilon: 0.01 } }
			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		Rectangle {
			x: toggle.dark && toggle.hovered ? 14.5 : 13.0
			y: toggle.dark && toggle.hovered ? 4.8 : 6.0
			width: 2.0
			height: 2.0
			radius: 0.5
			rotation: 45
			antialiasing: true
			color: Theme.text
			scale: toggle.dark && toggle.hovered ? 1 : 0
			opacity: toggle.dark && toggle.hovered ? 0.68 : 0

			Behavior on x { SpringAnimation { spring: 3.6; damping: 0.30; epsilon: 0.05 } }
			Behavior on y { SpringAnimation { spring: 3.6; damping: 0.30; epsilon: 0.05 } }
			Behavior on scale { SpringAnimation { spring: 3.8; damping: 0.28; epsilon: 0.01 } }
			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}
	}
}
