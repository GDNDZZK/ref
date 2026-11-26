pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// A round button with one centered glyph. Set Accessible.name on each use.
// Hero icons are drawn as independent multi-part vector glyphs whose parts animate with
// interruptible SpringAnimations to physically anticipate the button's action; other icons
// render via the SVG Icon provider with tailored spring micro-motions.
CircleButton {
	id: iconButton

	property string iconName: ""
	property int iconSize: 20
	property color iconColor: Theme.textMuted
	property int buttonSize: Theme.buttonHeight
	property string tooltipText: ""

	readonly property bool isClose: iconName === "close"
	readonly property bool isDangerIcon: isClose || iconName === "trash"
	readonly property bool isAccentActive: Qt.colorEqual(iconColor, Theme.accentText)
	readonly property bool isMutedDefault: Qt.colorEqual(iconColor, Theme.textMuted)
	readonly property real unitScale: iconSize / 20.0
	readonly property real strokeWidth: iconSize >= 18 ? 2.0 : 1.5
	readonly property bool isHeroVector: iconName === "float"
		|| iconName === "split"
		|| iconName === "sidebar"
		|| iconName === "maximize"
		|| iconName === "restore"
		|| iconName === "plus"
		|| iconName === "minus"
		|| iconName === "close"

	readonly property real fallbackShiftX: hovered && iconName === "chat-list" ? -1.8 : 0
	readonly property real fallbackShiftY: !hovered ? 0
		: (iconName === "stash" || iconName === "stash-count" ? 1.6
		: (iconName === "folder" || iconName === "folder-open" ? -1.4
		: (iconName === "trash" ? -1.0
		: (iconName === "pin" ? 1.0 : 0))))
	readonly property real fallbackRotation: !hovered ? 0
		: (iconName === "folder" || iconName === "folder-open" ? -7
		: (iconName === "bell" ? -14
		: (iconName === "retry" ? 45
		: (iconName === "trash" ? -10
		: (iconName === "pin" ? -12 : 0)))))
	readonly property real fallbackScale: pressed ? 0.94
		: (!hovered ? 1.0
		: (iconName === "chat-list" ? 1.06
		: (iconName === "stash" || iconName === "stash-count"
			|| iconName === "folder" || iconName === "folder-open"
			|| iconName === "bell" || iconName === "retry"
			|| iconName === "trash" || iconName === "pin" ? 1.08 : 1.0)))

	rimColor: iconButton.isDangerIcon ? Theme.danger : (Theme.dark ? Qt.rgba(0.68, 0.80, 1.0, 1.0) : Theme.accent)
	rimCoreColor: iconButton.isDangerIcon ? Theme.dangerText : (Theme.dark ? Qt.rgba(0.95, 0.97, 1.0, 1.0) : Theme.accentText)

	implicitWidth: buttonSize
	implicitHeight: buttonSize

	function heroComponentFor(name) {
		switch (name) {
		case "float": return floatGlyph
		case "split": return splitGlyph
		case "sidebar": return sidebarGlyph
		case "maximize": return maximizeGlyph
		case "restore": return restoreGlyph
		case "plus": return plusGlyph
		case "minus": return minusGlyph
		case "close": return closeGlyph
		default: return null
		}
	}

	Loader {
		id: vectorGlyphLoader
		anchors.centerIn: parent
		width: iconButton.iconSize
		height: iconButton.iconSize
		active: iconButton.isHeroVector
		sourceComponent: iconButton.heroComponentFor(iconButton.iconName)
	}

	Icon {
		id: fallbackIcon
		anchors.centerIn: parent
		visible: !iconButton.isHeroVector
		name: iconButton.isHeroVector ? "" : iconButton.iconName
		size: iconButton.iconSize
		color: iconButton.iconColor
		smooth: true
		antialiasing: true
		rotation: iconButton.fallbackRotation
		scale: iconButton.fallbackScale

		Behavior on rotation { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
		Behavior on scale { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.002 } }

		transform: Translate {
			x: iconButton.fallbackShiftX
			y: iconButton.fallbackShiftY

			Behavior on x { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
			Behavior on y { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
		}
	}

	PlainToolTip {
		parent: iconButton
		visible: iconButton.hovered && iconButton.tooltipText !== ""
		text: iconButton.tooltipText
		delay: 400
	}

	// 1. Float: window frame shrinks & recedes while the floating orb swells and glides center.
	Component {
		id: floatGlyph

		Item {
			id: floatRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth

			Rectangle {
				x: 2 * floatRoot.u
				y: 2 * floatRoot.u
				width: 13 * floatRoot.u
				height: 13 * floatRoot.u
				radius: 3 * floatRoot.u
				color: "transparent"
				border.width: floatRoot.st
				border.color: iconButton.iconColor
				scale: iconButton.hovered ? 0.72 : 1.0
				opacity: iconButton.hovered ? 0.38 : 1.0

				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on scale { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.002 } }
				Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

				transform: Translate {
					x: iconButton.hovered ? 2.2 * floatRoot.u : 0
					y: iconButton.hovered ? 2.2 * floatRoot.u : 0

					Behavior on x { SpringAnimation { spring: 4.6; damping: 0.30; epsilon: 0.05 } }
					Behavior on y { SpringAnimation { spring: 4.6; damping: 0.30; epsilon: 0.05 } }
				}
			}

			Rectangle {
				id: floatOrb
				readonly property real orbSize: 7.6 * floatRoot.u
				x: (iconButton.hovered ? 10.0 : 14.0) * floatRoot.u - orbSize * 0.5
				y: (iconButton.hovered ? 10.0 : 14.0) * floatRoot.u - orbSize * 0.5
				width: orbSize
				height: orbSize
				radius: orbSize * 0.5
				color: iconButton.color
				border.width: floatRoot.st
				border.color: iconButton.hovered ? Theme.accentText : iconButton.iconColor
				scale: iconButton.pressed ? 1.24 : (iconButton.hovered ? 1.48 : 1.0)

				Behavior on x { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
				Behavior on y { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
				Behavior on scale { SpringAnimation { spring: 5.0; damping: 0.26; epsilon: 0.002 } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Rectangle {
					anchors { fill: parent; margins: floatRoot.st }
					radius: width * 0.5
					color: Theme.accentText
					opacity: iconButton.hovered ? 0.24 : 0

					Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
			}
		}
	}

	// 2. Split: 4 independent tiles spread outward to split, or pull inward when already split.
	Component {
		id: splitGlyph

		Item {
			id: splitRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: Math.max(1.5, iconButton.strokeWidth * 0.9)
			readonly property real tileSize: 7.0 * u
			readonly property real baseOffset: 4.0 * u
			readonly property real hoverDelta: iconButton.isAccentActive ? -1.2 * u : 1.6 * u

			Repeater {
				model: 4

				Rectangle {
					required property int index
					readonly property real sx: (index % 2 === 0) ? -1 : 1
					readonly property real sy: (index < 2) ? -1 : 1
					readonly property real offset: splitRoot.baseOffset + (iconButton.hovered ? splitRoot.hoverDelta : 0)

					width: splitRoot.tileSize
					height: splitRoot.tileSize
					x: splitRoot.width * 0.5 + sx * offset - width * 0.5
					y: splitRoot.height * 0.5 + sy * offset - height * 0.5
					radius: 2.0 * splitRoot.u
					color: "transparent"
					border.width: splitRoot.st
					border.color: iconButton.iconColor
					scale: iconButton.hovered ? (iconButton.isAccentActive ? 0.96 : 1.06) : 1.0

					Behavior on x { SpringAnimation { spring: 5.0; damping: 0.28; epsilon: 0.05 } }
					Behavior on y { SpringAnimation { spring: 5.0; damping: 0.28; epsilon: 0.05 } }
					Behavior on scale { SpringAnimation { spring: 5.0; damping: 0.28; epsilon: 0.002 } }
					Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
			}
		}
	}

	// 3. Sidebar: outer frame with an independent vertical divider + softly tinted panel sliding out.
	Component {
		id: sidebarGlyph

		Item {
			id: sidebarRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth

			Rectangle {
				x: 2 * sidebarRoot.u + sidebarRoot.st * 0.7
				y: 3 * sidebarRoot.u + sidebarRoot.st * 0.7
				width: Math.max(0, sidebarDivider.x - x + sidebarRoot.st * 0.5)
				height: 14 * sidebarRoot.u - sidebarRoot.st * 1.4
				radius: 1.4 * sidebarRoot.u
				color: iconButton.iconColor
				opacity: iconButton.hovered ? 0.22 : 0.08

				Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}

			Rectangle {
				x: 2 * sidebarRoot.u
				y: 3 * sidebarRoot.u
				width: 16 * sidebarRoot.u
				height: 14 * sidebarRoot.u
				radius: 3 * sidebarRoot.u
				color: "transparent"
				border.width: sidebarRoot.st
				border.color: iconButton.iconColor

				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}

			Rectangle {
				id: sidebarDivider
				x: (iconButton.hovered ? 8.4 : 6.0) * sidebarRoot.u
				y: 3 * sidebarRoot.u
				width: sidebarRoot.st
				height: 14 * sidebarRoot.u
				radius: sidebarRoot.st * 0.5
				color: iconButton.iconColor

				Behavior on x { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	// 4. Maximize: 4 independent L-shaped corner brackets pushing radially outward on hover.
	Component {
		id: maximizeGlyph

		Item {
			id: maximizeRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth
			readonly property real armLen: 6.2 * u

			Repeater {
				model: 4

				Item {
					id: corner
					required property int index
					readonly property real sx: (index % 2 === 0) ? -1 : 1
					readonly property real sy: (index < 2) ? -1 : 1
					readonly property real spread: iconButton.pressed ? -0.4 * maximizeRoot.u : (iconButton.hovered ? 1.8 * maximizeRoot.u : 0)

					width: maximizeRoot.armLen
					height: maximizeRoot.armLen
					x: (sx < 0 ? 2 * maximizeRoot.u : maximizeRoot.width - 2 * maximizeRoot.u - width) + sx * spread
					y: (sy < 0 ? 2 * maximizeRoot.u : maximizeRoot.height - 2 * maximizeRoot.u - height) + sy * spread

					Behavior on x { SpringAnimation { spring: 5.2; damping: 0.28; epsilon: 0.05 } }
					Behavior on y { SpringAnimation { spring: 5.2; damping: 0.28; epsilon: 0.05 } }

					Rectangle {
						x: 0
						y: corner.sy < 0 ? 0 : corner.height - maximizeRoot.st
						width: maximizeRoot.armLen
						height: maximizeRoot.st
						radius: maximizeRoot.st * 0.5
						color: iconButton.iconColor

						Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					}

					Rectangle {
						x: corner.sx < 0 ? 0 : corner.width - maximizeRoot.st
						y: 0
						width: maximizeRoot.st
						height: maximizeRoot.armLen
						radius: maximizeRoot.st * 0.5
						color: iconButton.iconColor

						Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
					}
				}
			}
		}
	}

	// 5. Restore: 2 independent overlapping window frames gliding inward together on hover.
	Component {
		id: restoreGlyph

		Item {
			id: restoreRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth
			readonly property real frameSize: 11 * u

			Rectangle {
				x: (iconButton.hovered ? 5.5 : 6.5) * restoreRoot.u
				y: (iconButton.hovered ? 3.5 : 2.5) * restoreRoot.u
				width: restoreRoot.frameSize
				height: restoreRoot.frameSize
				radius: 2.5 * restoreRoot.u
				color: "transparent"
				border.width: restoreRoot.st
				border.color: iconButton.iconColor

				Behavior on x { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.05 } }
				Behavior on y { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.05 } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}

			Rectangle {
				x: (iconButton.hovered ? 3.5 : 2.5) * restoreRoot.u
				y: (iconButton.hovered ? 5.5 : 6.5) * restoreRoot.u
				width: restoreRoot.frameSize
				height: restoreRoot.frameSize
				radius: 2.5 * restoreRoot.u
				color: iconButton.color
				border.width: restoreRoot.st
				border.color: iconButton.iconColor
				scale: iconButton.hovered ? 0.94 : 1.0

				Behavior on x { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
				Behavior on y { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
				Behavior on scale { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.002 } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	// 6. Plus: 2 independent rounded bars rotating +90° and elongating on hover.
	Component {
		id: plusGlyph

		Item {
			id: plusRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth
			readonly property real barLen: (iconButton.hovered ? 15.5 : 13.5) * u

			rotation: iconButton.hovered ? 90 : 0
			Behavior on rotation { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }

			Rectangle {
				anchors.centerIn: parent
				width: plusRoot.barLen
				height: plusRoot.st
				radius: plusRoot.st * 0.5
				color: iconButton.iconColor

				Behavior on width { SpringAnimation { spring: 5.2; damping: 0.26; epsilon: 0.05 } }
				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}

			Rectangle {
				anchors.centerIn: parent
				width: plusRoot.st
				height: plusRoot.barLen
				radius: plusRoot.st * 0.5
				color: iconButton.iconColor

				Behavior on height { SpringAnimation { spring: 5.2; damping: 0.26; epsilon: 0.05 } }
				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	// 7. Minus: horizontal bar sliding downward toward the bottom edge on hover.
	Component {
		id: minusGlyph

		Item {
			id: minusRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth

			Rectangle {
				anchors.horizontalCenter: parent.horizontalCenter
				y: (minusRoot.height - height) * 0.5 + (iconButton.hovered ? 3.5 * minusRoot.u : 0)
				width: 12.5 * minusRoot.u
				height: minusRoot.st
				radius: minusRoot.st * 0.5
				color: iconButton.iconColor

				Behavior on y { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.05 } }
				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}

	// 8. Close: 2 independent crossing bars rotating +90° and snapping crisply in danger red.
	Component {
		id: closeGlyph

		Item {
			id: closeRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth
			readonly property color strokeColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.dangerText : iconButton.iconColor

			rotation: iconButton.hovered ? 90 : 0
			scale: iconButton.pressed ? 0.88 : (iconButton.hovered ? 1.08 : 1.0)

			Behavior on rotation { SpringAnimation { spring: 5.4; damping: 0.28; epsilon: 0.05 } }
			Behavior on scale { SpringAnimation { spring: 5.4; damping: 0.28; epsilon: 0.002 } }

			Rectangle {
				anchors.centerIn: parent
				width: 14.5 * closeRoot.u
				height: closeRoot.st
				radius: closeRoot.st * 0.5
				rotation: 45
				antialiasing: true
				color: closeRoot.strokeColor

				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}

			Rectangle {
				anchors.centerIn: parent
				width: 14.5 * closeRoot.u
				height: closeRoot.st
				radius: closeRoot.st * 0.5
				rotation: -45
				antialiasing: true
				color: closeRoot.strokeColor

				Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			}
		}
	}
}
