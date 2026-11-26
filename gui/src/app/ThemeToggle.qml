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
	property bool holdLightSurface: false
	readonly property bool visualDark: toggle.dark && !toggle.holdLightSurface
	readonly property color glyphColor: toggle.holdLightSurface ? "#1d1d1f" : Theme.text
	property real sunriseProgress: 1.0
	property real sunsetProgress: 1.0
	property bool initialized: false
	readonly property bool sunriseActive: !visualDark && sunriseProgress < 1.0
	readonly property bool sunsetActive: visualDark && sunsetProgress < 1.0
	readonly property bool morphActive: sunriseActive || sunsetActive
	readonly property real sunriseStarFade: 1.0 - smoothstep(0.0, 0.14, sunriseProgress)
	readonly property real biteOut: smoothstep(0.0, 0.24, sunriseProgress)
	readonly property real discShrink: smoothstep(0.20, 0.48, sunriseProgress)
	readonly property real rayEmerge: smoothstep(0.20, 0.48, sunriseProgress)
	readonly property real rayFadeIn: smoothstep(0.18, 0.36, sunriseProgress)
	readonly property real bloomWave: sunriseActive && sunriseProgress > 0.18
		? Math.sin(Math.PI * Math.pow((sunriseProgress - 0.18) / 0.82, 0.62))
		: 0.0
	readonly property real hoverBlend: smoothstep(0.46, 1.0, sunriseProgress)
	readonly property real rotEmerge: smoothstep(0.0, 0.24, sunriseProgress)
	// Light -> Dark (sun -> full disc -> crescent moon + stars)
	readonly property real rayRetract: smoothstep(0.0, 0.30, sunsetProgress)
	readonly property real discExpand: smoothstep(0.0, 0.30, sunsetProgress)
	readonly property real biteIn: smoothstep(0.24, 0.68, sunsetProgress)
	readonly property real sunsetStarIn: smoothstep(0.55, 1.0, sunsetProgress)
	readonly property real sunsetRotT: smoothstep(0.15, 0.75, sunsetProgress)

	function smoothstep(edge0, edge1, x) {
		const t = Math.max(0, Math.min(1, (x - edge0) / (edge1 - edge0)))
		return t * t * (3 - 2 * t)
	}

	forceLightSurface: toggle.holdLightSurface
	rimColor: toggle.visualDark ? Qt.rgba(0.76, 0.88, 1.0, 1.0) : Qt.rgba(0.16, 0.18, 0.36, 1.0)
	rimCoreColor: toggle.visualDark ? Qt.rgba(0.98, 0.99, 1.0, 1.0) : Qt.rgba(0.06, 0.08, 0.18, 1.0)
	rimBiasAngle: toggle.visualDark ? Math.PI * 0.78 : -Math.PI * 0.25
	rimStrengthScale: toggle.visualDark ? 1.45 : 0.45
	eclipseShade: toggle.visualDark ? 0.0 : 1.35 * toggle.hoverBlend

	Accessible.name: toggle.dark ? qsTr("Switch to light theme") : qsTr("Switch to dark theme")

	Component.onCompleted: initialized = true

	NumberAnimation {
		id: sunriseAnim
		target: toggle
		property: "sunriseProgress"
		from: 0.0
		to: 1.0
		duration: 580
		easing.type: Easing.Linear
	}
	NumberAnimation {
		id: sunsetAnim
		target: toggle
		property: "sunsetProgress"
		from: 0.0
		to: 1.0
		duration: 540
		easing.type: Easing.Linear
	}

	onVisualDarkChanged: {
		if (!initialized)
			return
		if (!visualDark) {
			sunsetAnim.stop()
			sunsetProgress = 1.0
			sunriseProgress = 0.0
			sunriseAnim.restart()
		} else {
			sunriseAnim.stop()
			sunriseProgress = 1.0
			sunsetProgress = 0.0
			sunsetAnim.restart()
		}
	}

	Item {
		id: glyph
		anchors.centerIn: parent
		width: 16
		height: 16
		readonly property real lightRot: (toggle.hovered ? -22 * toggle.hoverBlend : 0) + 18 * toggle.bloomWave
		readonly property real lightScale: 1.0 + (toggle.hovered ? -0.04 * toggle.hoverBlend : 0)
		rotation: toggle.sunsetActive
			? (toggle.hovered ? -22 : 0) * (1 - toggle.sunsetRotT) + (toggle.hovered ? -12 : 0) * toggle.sunsetRotT
			: (toggle.visualDark
				? (toggle.hovered ? -12 : 0)
				: (toggle.sunriseActive
					? (toggle.hovered ? -12 : 0) * (1 - toggle.rotEmerge) + lightRot * toggle.rotEmerge
					: lightRot))
		scale: toggle.sunsetActive
			? (toggle.hovered ? 0.96 : 1.0) * (1 - toggle.sunsetRotT) + (toggle.hovered ? 1.08 : 1.0) * toggle.sunsetRotT
			: (toggle.visualDark
				? (toggle.hovered ? 1.08 : 1.0)
				: (toggle.sunriseActive
					? (toggle.hovered ? 1.08 : 1.0) * (1 - toggle.rotEmerge) + lightScale * toggle.rotEmerge
					: lightScale))

		Behavior on rotation {
			enabled: !toggle.morphActive
			SpringAnimation {
				spring: 4.5
				damping: 0.28
				epsilon: 0.05
			}
		}
		Behavior on scale {
			enabled: !toggle.morphActive
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
					readonly property real lightY: -1.0 + (toggle.hovered ? 2.5 * toggle.hoverBlend : 0.0) - 1.8 * toggle.bloomWave
					readonly property real lightOpacity: 1.0 - (toggle.hovered ? 0.48 * toggle.hoverBlend : 0.0)
					readonly property real sunsetStartY: toggle.hovered ? 1.5 : -1.0
					readonly property real sunsetStartOp: toggle.hovered ? 0.52 : 1.0
					x: (parent.width - width) / 2
					y: toggle.sunsetActive
						? sunsetStartY + (6.0 - sunsetStartY) * toggle.rayRetract
						: (toggle.visualDark ? 6.0 : 6.0 + (lightY - 6.0) * toggle.rayEmerge)
					width: 2
					height: 4
					radius: 1
					color: toggle.glyphColor
					opacity: toggle.sunsetActive
						? sunsetStartOp * (1.0 - toggle.rayRetract)
						: (toggle.visualDark ? 0.0 : lightOpacity * toggle.rayFadeIn)

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					Behavior on y {
						enabled: !toggle.morphActive
						SpringAnimation {
							spring: 4.6
							damping: 0.32
							epsilon: 0.05
						}
					}
					Behavior on opacity {
						enabled: !toggle.morphActive
						NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
					}
				}
			}
		}

		// Center disc: stays full 14px during moon -> full disc, then condenses cleanly to 8px sun core (and symmetrically in reverse).
		Rectangle {
			anchors.centerIn: parent
			width: toggle.sunriseActive
				? 14.0 - 6.0 * toggle.discShrink
				: (toggle.sunsetActive
					? 8.0 + 6.0 * toggle.discExpand
					: (toggle.visualDark ? 14.0 : 8.0))
			height: width
			radius: width / 2
			color: toggle.glyphColor

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		// Painted in the button's own color, so it reads as a cut-out.
		Rectangle {
			x: toggle.sunriseActive
				? 6.0 + 7.0 * toggle.biteOut
				: (toggle.sunsetActive
					? 13.0 - 7.0 * toggle.biteIn
					: (toggle.visualDark ? 6.0 : 13.0))
			y: toggle.sunriseActive
				? -2.0 - 7.0 * toggle.biteOut
				: (toggle.sunsetActive
					? -9.0 + 7.0 * toggle.biteIn
					: (toggle.visualDark ? -2.0 : -9.0))
			width: 11
			height: 11
			radius: 5.5
			color: toggle.color
			scale: toggle.sunriseActive
				? 1.0 - toggle.biteOut
				: (toggle.sunsetActive
					? toggle.biteIn
					: (toggle.visualDark ? 1.0 : 0.0))
		}

		// Subtle dawn-drifting star glints beside the crescent in dark mode on hover.
		Rectangle {
			x: toggle.visualDark && toggle.hovered ? 12.2 : 10.6
			y: toggle.visualDark && toggle.hovered ? 0.8 : 2.2
			width: 2.8
			height: 2.8
			radius: 0.6
			rotation: 45
			antialiasing: true
			color: toggle.glyphColor
			scale: toggle.sunriseActive
				? (toggle.hovered ? toggle.sunriseStarFade : 0.0)
				: (toggle.sunsetActive
					? (toggle.hovered ? toggle.sunsetStarIn : 0.0)
					: (toggle.visualDark && toggle.hovered ? 1.0 : 0.0))
			opacity: toggle.sunriseActive
				? (toggle.hovered ? 0.88 * toggle.sunriseStarFade : 0.0)
				: (toggle.sunsetActive
					? (toggle.hovered ? 0.88 * toggle.sunsetStarIn : 0.0)
					: (toggle.visualDark && toggle.hovered ? 0.88 : 0.0))

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on x { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
			Behavior on y { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
			Behavior on scale { enabled: !toggle.morphActive; NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on opacity { enabled: !toggle.morphActive; NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		Rectangle {
			x: toggle.visualDark && toggle.hovered ? 14.5 : 13.0
			y: toggle.visualDark && toggle.hovered ? 4.8 : 6.0
			width: 2.0
			height: 2.0
			radius: 0.5
			rotation: 45
			antialiasing: true
			color: toggle.glyphColor
			scale: toggle.sunriseActive
				? (toggle.hovered ? toggle.sunriseStarFade : 0.0)
				: (toggle.sunsetActive
					? (toggle.hovered ? toggle.sunsetStarIn : 0.0)
					: (toggle.visualDark && toggle.hovered ? 1.0 : 0.0))
			opacity: toggle.sunriseActive
				? (toggle.hovered ? 0.68 * toggle.sunriseStarFade : 0.0)
				: (toggle.sunsetActive
					? (toggle.hovered ? 0.68 * toggle.sunsetStarIn : 0.0)
					: (toggle.visualDark && toggle.hovered ? 0.68 : 0.0))

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on x { SpringAnimation { spring: 3.6; damping: 0.30; epsilon: 0.05 } }
			Behavior on y { SpringAnimation { spring: 3.6; damping: 0.30; epsilon: 0.05 } }
			Behavior on scale { enabled: !toggle.morphActive; NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			Behavior on opacity { enabled: !toggle.morphActive; NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}
	}
}
