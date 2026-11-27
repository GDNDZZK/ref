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
	property string badgeText: ""

	readonly property bool isClose: iconName === "close"
	readonly property bool isCreateIcon: iconName === "plus"
	readonly property bool isDangerIcon: isClose || iconName === "trash"
	readonly property bool isWarningIcon: iconName === "retry" || iconName === "bell"
	readonly property bool isInfoIcon: iconName === "folder" || iconName === "folder-open" || iconName === "stash" || iconName === "stash-count" || iconName === "file-text"
	readonly property bool isNeutralIcon: iconName === "minus" || iconName === "bell-off"
	readonly property bool isAccentActive: Qt.colorEqual(iconColor, Theme.accentText)
	readonly property bool isMutedDefault: Qt.colorEqual(iconColor, Theme.textMuted)
	property real retrySpinAngle: 0
	readonly property real unitScale: iconSize / 20.0
	readonly property real strokeWidth: iconSize >= 18 ? 2.0 : 1.5
	readonly property bool isHeroVector: iconName === "float"
		|| iconName === "float-return"
		|| iconName === "split"
		|| iconName === "sidebar"
		|| iconName === "maximize"
		|| iconName === "restore"
		|| iconName === "plus"
		|| iconName === "minus"
		|| iconName === "close"
		|| iconName === "pin"
		|| iconName === "folder"
		|| iconName === "folder-open"
		|| iconName === "trash"
		|| iconName === "stash"
		|| iconName === "stash-count"
		|| iconName === "bell"
		|| iconName === "bell-off"
		|| iconName === "sliders"
		|| iconName === "file-text"
		|| iconName === "chat-list"
	readonly property real fallbackShiftX: 0
	readonly property real fallbackShiftY: 0
	readonly property real fallbackRotation: iconName === "retry"
		? retrySpinAngle + (hovered ? 45 : 0)
		: 0
	readonly property real fallbackScale: pressed ? 0.94
		: (!hovered ? 1.0 : (iconName === "retry" ? 1.08 : 1.0))
	readonly property real glyphPixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)

	function smoothstep(edge0, edge1, x) {
		const t = Math.max(0, Math.min(1, (x - edge0) / (edge1 - edge0)))
		return t * t * (3 - 2 * t)
	}
	function glyphRgba(c, alpha) {
		const r = Math.round(c.r * 255)
		const g = Math.round(c.g * 255)
		const b = Math.round(c.b * 255)
		const a = Math.max(0, Math.min(1, alpha))
		return "rgba(" + r + "," + g + "," + b + "," + a.toFixed(4) + ")"
	}
	rimStrengthScale: iconButton.isDangerIcon ? 0.56 : (iconButton.isCreateIcon || iconButton.isWarningIcon ? 0.42 : (iconButton.isNeutralIcon ? 0.26 : 0.36))
	rimColor: iconButton.isDangerIcon ? Theme.danger
		: (iconButton.isCreateIcon ? Theme.success
		: (iconButton.isWarningIcon ? Theme.warning
		: (iconButton.isInfoIcon ? Theme.info
		: (iconButton.isNeutralIcon ? Theme.textMuted : Theme.accent))))
	rimCoreColor: iconButton.isDangerIcon ? Theme.dangerText
		: (iconButton.isCreateIcon ? Theme.successText
		: (iconButton.isWarningIcon ? Theme.warningText
		: (iconButton.isInfoIcon ? Theme.infoText
		: (iconButton.isNeutralIcon ? Theme.text : Theme.accentText))))

	onClicked: if (iconName === "retry") retrySpinAngle += 360

	implicitWidth: buttonSize
	implicitHeight: buttonSize

	function heroComponentFor(name) {
		switch (name) {
		case "float":
		case "float-return":
			return floatGlyph
		case "split": return splitGlyph
		case "sidebar": return sidebarGlyph
		case "maximize":
		case "restore":
			return windowSizeGlyph
		case "plus": return plusGlyph
		case "minus": return minusGlyph
		case "close": return closeGlyph
		case "pin": return pinGlyph
		case "folder":
		case "folder-open":
			return folderGlyph
		case "trash": return trashGlyph
		case "stash":
		case "stash-count":
			return stashGlyph
		case "bell":
		case "bell-off":
			return bellGlyph
		case "sliders": return slidersGlyph
		case "file-text": return fileTextGlyph
		case "chat-list": return chatListGlyph
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

		Behavior on rotation {
			SpringAnimation {
				spring: iconButton.iconName === "retry" ? 3.4 : 4.8
				damping: iconButton.iconName === "retry" ? 0.34 : 0.28
				epsilon: iconButton.iconName === "retry" ? 0.1 : 0.05
			}
		}
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

	// 1. Float / Float-Return: mirrored window-frame <-> floating-orb morph.
	Component {
		id: floatGlyph

		Item {
			id: floatRoot
			anchors.fill: parent
			readonly property real u: iconButton.unitScale
			readonly property real st: iconButton.strokeWidth
			readonly property bool isReturn: iconButton.iconName === "float-return"

			Rectangle {
				id: floatFrame
				z: floatRoot.isReturn && iconButton.hovered ? 2 : 0
				x: 2 * floatRoot.u
				y: 2 * floatRoot.u
				width: 13 * floatRoot.u
				height: 13 * floatRoot.u
				radius: 3 * floatRoot.u
				color: "transparent"
				border.width: floatRoot.st
				border.color: floatRoot.isReturn && iconButton.hovered ? Theme.accentText : iconButton.iconColor
				scale: floatRoot.isReturn
					? (iconButton.pressed ? 0.96 : (iconButton.hovered ? 1.06 : 0.76))
					: (iconButton.hovered ? 0.72 : 1.0)
				opacity: floatRoot.isReturn
					? (iconButton.hovered ? 1.0 : 0.45)
					: (iconButton.hovered ? 0.38 : 1.0)

				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				Behavior on scale { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.002 } }
				Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

				transform: Translate {
					x: floatRoot.isReturn
						? (iconButton.hovered ? 1.5 * floatRoot.u : 0)
						: (iconButton.hovered ? 2.2 * floatRoot.u : 0)
					y: floatRoot.isReturn
						? (iconButton.hovered ? 1.5 * floatRoot.u : 0)
						: (iconButton.hovered ? 2.2 * floatRoot.u : 0)

					Behavior on x { SpringAnimation { spring: 4.6; damping: 0.30; epsilon: 0.05 } }
					Behavior on y { SpringAnimation { spring: 4.6; damping: 0.30; epsilon: 0.05 } }
				}

				Rectangle {
					anchors { fill: parent; margins: floatRoot.st }
					radius: Math.max(0.5, floatFrame.radius - floatRoot.st * 0.6)
					color: Theme.accentText
					opacity: floatRoot.isReturn && iconButton.hovered ? 0.14 : 0

					Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
			}

			Rectangle {
				id: floatOrb
				z: 1
				readonly property real orbSize: 7.6 * floatRoot.u
				x: (floatRoot.isReturn
					? (iconButton.hovered ? 14.2 : 10.0)
					: (iconButton.hovered ? 10.0 : 14.0)) * floatRoot.u - orbSize * 0.5
				y: (floatRoot.isReturn
					? (iconButton.hovered ? 14.2 : 10.0)
					: (iconButton.hovered ? 10.0 : 14.0)) * floatRoot.u - orbSize * 0.5
				width: orbSize
				height: orbSize
				radius: orbSize * 0.5
				color: iconButton.color
				border.width: floatRoot.st
				border.color: !floatRoot.isReturn && iconButton.hovered ? Theme.accentText : iconButton.iconColor
				scale: floatRoot.isReturn
					? (iconButton.hovered ? 0.76 : 1.15)
					: (iconButton.pressed ? 1.24 : (iconButton.hovered ? 1.48 : 1.0))
				opacity: floatRoot.isReturn && iconButton.hovered ? 0.42 : 1.0

				Behavior on x { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
				Behavior on y { SpringAnimation { spring: 4.6; damping: 0.28; epsilon: 0.05 } }
				Behavior on scale { SpringAnimation { spring: 5.0; damping: 0.26; epsilon: 0.002 } }
				Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Rectangle {
					anchors { fill: parent; margins: floatRoot.st }
					radius: width * 0.5
					color: Theme.accentText
					opacity: !floatRoot.isReturn && iconButton.hovered ? 0.24 : 0

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

	// 4. Window Size (Maximize <-> Restore): single continuous morph in [0, 1] rendered on HiDPI Canvas.
	// Corner brackets grow seamlessly from 34% to 50% arm length to fuse into a unified rounded frame while
	// smoothly splitting into the classic overlapping dual restore frames with zero midpoint pause.
	Component {
		id: windowSizeGlyph

		Item {
			id: windowSizeRoot
			anchors.fill: parent
			readonly property bool isRestore: iconButton.iconName === "restore"
			readonly property real pad: 4
			property real morph: isRestore ? 1.0 : 0.0
			property real hoverVal: iconButton.pressed ? -0.3 : (iconButton.hovered ? 1.0 : 0.0)
			property color strokeColor: iconButton.iconColor
			property color maskColor: iconButton.color

			Behavior on morph { NumberAnimation { duration: 420; easing.type: Easing.InOutCubic } }
			Behavior on hoverVal { SpringAnimation { spring: 5.0; damping: 0.30; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onMorphChanged: wsCanvas.requestPaint()
			onHoverValChanged: wsCanvas.requestPaint()
			onStrokeColorChanged: wsCanvas.requestPaint()
			onMaskColorChanged: wsCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { wsCanvas.requestPaint() }
			}

			Canvas {
				id: wsCanvas
				x: -windowSizeRoot.pad
				y: -windowSizeRoot.pad
				readonly property real logicalW: windowSizeRoot.width + 2 * windowSizeRoot.pad
				readonly property real logicalH: windowSizeRoot.height + 2 * windowSizeRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				function drawRoundedRect(ctx, x, y, w, h, r) {
					const rr = Math.min(r, w * 0.5, h * 0.5)
					ctx.beginPath()
					ctx.moveTo(x + rr, y)
					ctx.lineTo(x + w - rr, y)
					ctx.arcTo(x + w, y, x + w, y + rr, rr)
					ctx.lineTo(x + w, y + h - rr)
					ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr)
					ctx.lineTo(x + rr, y + h)
					ctx.arcTo(x, y + h, x, y + h - rr, rr)
					ctx.lineTo(x, y + rr)
					ctx.arcTo(x, y, x + rr, y, rr)
					ctx.closePath()
				}

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || windowSizeRoot.width <= 0 || windowSizeRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const u = windowSizeRoot.width / 20.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(windowSizeRoot.pad, windowSizeRoot.pad)
					ctx.scale(u, u)

					const m = Math.max(0, Math.min(1, windowSizeRoot.morph))
					const h = windowSizeRoot.hoverVal
					const tJoin = iconButton.smoothstep(0.0, 0.56, m)
					const tSplit = iconButton.smoothstep(0.42, 1.0, m)
					const hR = h * tSplit
					const spread = h * 1.4 * (1.0 - tJoin)

					const fx = (2.8 - spread) + ((2.5 + hR * 0.9) - (2.8 - spread)) * tSplit
					const fy = (2.8 - spread) + ((6.5 - hR * 0.9) - (2.8 - spread)) * tSplit
					const fw = (14.4 + 2.0 * spread) + (11.0 - (14.4 + 2.0 * spread)) * tSplit
					const fh = fw
					const fr = 2.2

					const bx = fx + ((6.5 - hR * 0.9) - fx) * tSplit
					const by = fy + ((2.5 + hR * 0.9) - fy) * tSplit
					const lw = iconButton.strokeWidth / Math.max(0.5, u)

					ctx.lineWidth = lw
					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					// 1. Back restore frame sliding out to upper-right.
					const backAlpha = iconButton.smoothstep(0.46, 0.85, m)
					if (backAlpha > 0.005) {
						ctx.strokeStyle = iconButton.glyphRgba(windowSizeRoot.strokeColor, backAlpha)
						drawRoundedRect(ctx, bx, by, fw, fh, fr)
						ctx.stroke()
					}

					// 2. Front frame interior mask occluding the overlapping back frame lines.
					const fillAlpha = iconButton.smoothstep(0.46, 0.72, m)
					if (fillAlpha > 0.005) {
						ctx.fillStyle = iconButton.glyphRgba(windowSizeRoot.maskColor, fillAlpha)
						drawRoundedRect(ctx, fx, fy, fw, fh, fr)
						ctx.fill()
					}

					// 3. Continuous corner brackets (armFrac 0.34 -> 0.50 seamlessly closes into full rounded frame).
					const arm = fw * (0.34 + 0.16 * tJoin)
					const rr = Math.min(fr, arm)
					ctx.strokeStyle = iconButton.glyphRgba(windowSizeRoot.strokeColor, 1.0)
					ctx.beginPath()
					// Top-left corner
					ctx.moveTo(fx, fy + arm)
					ctx.lineTo(fx, fy + rr)
					ctx.arcTo(fx, fy, fx + rr, fy, rr)
					ctx.lineTo(fx + arm, fy)
					// Top-right corner
					ctx.moveTo(fx + fw - arm, fy)
					ctx.lineTo(fx + fw - rr, fy)
					ctx.arcTo(fx + fw, fy, fx + fw, fy + rr, rr)
					ctx.lineTo(fx + fw, fy + arm)
					// Bottom-right corner
					ctx.moveTo(fx + fw, fy + fh - arm)
					ctx.lineTo(fx + fw, fy + fh - rr)
					ctx.arcTo(fx + fw, fy + fh, fx + fw - rr, fy + fh, rr)
					ctx.lineTo(fx + fw - arm, fy + fh)
					// Bottom-left corner
					ctx.moveTo(fx + arm, fy + fh)
					ctx.lineTo(fx + rr, fy + fh)
					ctx.arcTo(fx, fy + fh, fx, fy + fh - rr, rr)
					ctx.lineTo(fx, fy + fh - arm)
					ctx.stroke()
				}
			}
		}
	}
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

	// 9. Pin: HiDPI Canvas 1:1 reproduction of assets/icons/pin-16.svg with sleek diagonal push-in, solid fill & surface slot.
	Component {
		id: pinGlyph

		Item {
			id: pinRoot
			anchors.fill: parent
			readonly property real pad: 3

			readonly property bool pinning: iconButton.isAccentActive
			property bool justUnpinned: false
			property bool justPinned: false
			property real pinProgress: pinning ? 1.0 : 0.0
			property real aimT: (pinning || (iconButton.hovered && !justUnpinned)) ? 1.0 : 0.0
			property real hoverVal: iconButton.pressed ? -0.4 : (iconButton.hovered ? 1.0 : 0.0)
			property real pullPreviewT: pinning && iconButton.hovered && !justPinned ? 1.0 : 0.0
			property real tiltX: pinning && iconButton.hovered && !justPinned ? iconButton.pointerNormX : 0.0
			property color strokeColor: iconButton.iconColor

			onPinningChanged: {
				if (!pinning && iconButton.hovered) {
					justUnpinned = true
					justPinned = false
				} else if (pinning) {
					justUnpinned = false
					if (iconButton.hovered)
						justPinned = true
				}
			}

			Behavior on pinProgress {
				NumberAnimation {
					duration: pinRoot.pinning ? 340 : 420
					easing.type: Easing.Linear
				}
			}
			Behavior on aimT { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.004 } }
			Behavior on hoverVal { SpringAnimation { spring: 5.0; damping: 0.30; epsilon: 0.005 } }
			Behavior on pullPreviewT { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.005 } }
			Behavior on tiltX { SpringAnimation { spring: 4.5; damping: 0.30; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onPinProgressChanged: pinCanvas.requestPaint()
			onAimTChanged: pinCanvas.requestPaint()
			onHoverValChanged: pinCanvas.requestPaint()
			onPullPreviewTChanged: pinCanvas.requestPaint()
			onTiltXChanged: pinCanvas.requestPaint()
			onStrokeColorChanged: pinCanvas.requestPaint()

			Connections {
				target: iconButton
				function onHoveredChanged() {
					if (!iconButton.hovered) {
						pinRoot.justUnpinned = false
						pinRoot.justPinned = false
					}
				}
				function onGlyphPixelScaleChanged() { pinCanvas.requestPaint() }
			}

			Canvas {
				id: pinCanvas
				x: -pinRoot.pad
				y: -pinRoot.pad
				readonly property real logicalW: pinRoot.width + 2 * pinRoot.pad
				readonly property real logicalH: pinRoot.height + 2 * pinRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || pinRoot.width <= 0 || pinRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const s = pinRoot.width / 16.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(pinRoot.pad, pinRoot.pad)
					ctx.scale(s, s)
					const p = Math.max(0, Math.min(1, pinRoot.pinProgress))
					const aim = Math.max(0, Math.min(1.2, pinRoot.aimT))
					const h = pinRoot.hoverVal
					const pullPreview = Math.max(0, Math.min(1.2, pinRoot.pullPreviewT))
					const pinning = pinRoot.pinning
					const rotT = pinning ? iconButton.smoothstep(0.0, 0.35, p) : iconButton.smoothstep(0.0, 0.62, p)
					const effectiveRotT = Math.min(1.0, Math.max(rotT, aim))
					const embedT = pinning ? iconButton.smoothstep(0.08, 0.88, p) : iconButton.smoothstep(0.42, 1.0, p)
					const slotT = pinning ? iconButton.smoothstep(0.24, 0.90, p) : iconButton.smoothstep(0.50, 0.96, p)
					const impulseY = pinning
						? 0.35 * Math.sin(Math.PI * iconButton.smoothstep(0.62, 1.0, p))
						: -1.7 * Math.sin(Math.PI * iconButton.smoothstep(0.18, 0.88, 1.0 - p))

					ctx.lineCap = "round"
					ctx.lineJoin = "round"
					// 1. Delicate surface landing line centered at x = 8.0 where the vertical needle embeds.
					if (slotT > 0.01) {
						const slotW = 6.0 * slotT
						ctx.strokeStyle = iconButton.glyphRgba(pinRoot.strokeColor, 0.78 * slotT)
						ctx.lineWidth = 1.35
						ctx.beginPath()
						ctx.moveTo(8.0 - slotW * 0.5, 14.15)
						ctx.lineTo(8.0 + slotW * 0.5, 14.15)
						ctx.stroke()
					}

					// 2. Pure linear outline pushpin (exact assets/icons/pin-16.svg coordinates).
					// Unpinned hover aims upright (-45°, y -0.8u); click drives straight down without wobble; unpin pulls straight up then relaxes to 45°.
					const aimLiftY = -0.8 * aim * (1.0 - embedT)
					const pinnedHoverY = -1.1 * embedT * pullPreview * Math.max(0, h)
					const dy = 0.75 * embedT + impulseY + aimLiftY + pinnedHoverY
					const angleDeg = -45.0 * effectiveRotT + 14.0 * pinRoot.tiltX * embedT * pullPreview
					ctx.save()
					ctx.translate(8.0, 8.0 + dy)
					ctx.rotate(angleDeg * Math.PI / 180)
					ctx.translate(-8.0, -8.0)

					// Slender needle tip embeds into the surface while pinned and emerges cleanly during vertical pull-out.
					const tipPull = embedT * pullPreview * Math.max(0, h) * 0.65
					const tipX = 2.25 + (4.2 - 2.25) * embedT - tipPull
					const tipY = 13.75 + (11.8 - 13.75) * embedT + tipPull
					ctx.strokeStyle = iconButton.glyphRgba(pinRoot.strokeColor, 1.0)
					ctx.lineWidth = 1.5
					ctx.beginPath()
					ctx.moveTo(6.5, 9.5)
					ctx.lineTo(tipX, tipY)
					ctx.stroke()

					// Pure linear outline polygon: M9.75 2.25 l4 4 -2 1 -2.5 2.5 v2.5 l-5.5 -5.5 h2.5 l2.5 -2.5 z
					ctx.beginPath()
					ctx.moveTo(9.75, 2.25)
					ctx.lineTo(13.75, 6.25)
					ctx.lineTo(11.75, 7.25)
					ctx.lineTo(9.25, 9.75)
					ctx.lineTo(9.25, 12.25)
					ctx.lineTo(3.75, 6.75)
					ctx.lineTo(6.25, 6.75)
					ctx.lineTo(8.75, 4.25)
					ctx.closePath()
					ctx.stroke()
					ctx.restore()
				}
			}
		}
	}

	// 10. Folder / Folder-Open: HiDPI Canvas 3-layer physical sandwich driven by a single continuous `open` spring:
	// Layer 1 (tabbed back plate) -> Layer 2 (rising document sheet) -> Layer 3 (perspective hinged front flap).
	Component {
		id: folderGlyph

		Item {
			id: folderRoot
			anchors.fill: parent
			readonly property real pad: 4
			property real open: iconButton.pressed ? 0.35 : (iconButton.hovered ? 1.0 : 0.0)
			property color strokeColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.infoText : iconButton.iconColor
			property color maskColor: iconButton.color
			property color tintColor: Theme.info

			Behavior on open { SpringAnimation { spring: 4.5; damping: 0.30; epsilon: 0.004 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onOpenChanged: folderCanvas.requestPaint()
			onStrokeColorChanged: folderCanvas.requestPaint()
			onMaskColorChanged: folderCanvas.requestPaint()
			onTintColorChanged: folderCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { folderCanvas.requestPaint() }
			}

			Canvas {
				id: folderCanvas
				x: -folderRoot.pad
				y: -folderRoot.pad
				readonly property real logicalW: folderRoot.width + 2 * folderRoot.pad
				readonly property real logicalH: folderRoot.height + 2 * folderRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				function drawRoundedRect(ctx, x, y, w, h, r) {
					const rr = Math.min(r, w * 0.5, h * 0.5)
					ctx.beginPath()
					ctx.moveTo(x + rr, y)
					ctx.lineTo(x + w - rr, y)
					ctx.arcTo(x + w, y, x + w, y + rr, rr)
					ctx.lineTo(x + w, y + h - rr)
					ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr)
					ctx.lineTo(x + rr, y + h)
					ctx.arcTo(x, y + h, x, y + h - rr, rr)
					ctx.lineTo(x, y + rr)
					ctx.arcTo(x, y, x + rr, y, rr)
					ctx.closePath()
				}

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || folderRoot.width <= 0 || folderRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const u = folderRoot.width / 20.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(folderRoot.pad, folderRoot.pad)
					ctx.scale(u, u)

					ctx.lineCap = "round"
					ctx.lineJoin = "round"
					const ov = Math.max(0, Math.min(1.25, folderRoot.open))
					const ovClamped = Math.min(1.0, ov)

					// Layer 1 (always back): single continuous tabbed folder contour (exact folder-20.svg proportion).
					ctx.beginPath()
					ctx.moveTo(3.0, 5.8)
					ctx.arcTo(3.0, 4.0, 4.8, 4.0, 1.8)
					ctx.lineTo(8.0, 4.0)
					ctx.lineTo(9.8, 6.0)
					ctx.lineTo(15.2, 6.0)
					ctx.arcTo(17.0, 6.0, 17.0, 7.8, 1.8)
					ctx.lineTo(17.0, 14.8)
					ctx.arcTo(17.0, 16.6, 15.2, 16.6, 1.8)
					ctx.lineTo(4.8, 16.6)
					ctx.arcTo(3.0, 16.6, 3.0, 14.8, 1.8)
					ctx.closePath()
					ctx.fillStyle = iconButton.glyphRgba(folderRoot.maskColor, 1.0)
					ctx.fill()
					ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, 1.0)
					ctx.lineWidth = 1.75
					ctx.stroke()

					// Layer 2 (sandwiched): document sheet gliding smoothly out from behind Layer 3.
					const sheetAlpha = iconButton.smoothstep(0.14, 0.42, ov)
					if (sheetAlpha > 0.01) {
						const sheetW = 9.4
						const sheetH = 7.6
						const sheetX = (20.0 - sheetW) * 0.5
						const sheetY = 10.6 + (4.2 - 10.6) * ov
						const sheetRot = -3.0 * ov
						ctx.save()
						// Clip strictly above the folder bottom edge so the tucked sheet never leaks below y = 16.4.
						ctx.beginPath()
						ctx.rect(-4.0, -4.0, 28.0, 20.4)
						ctx.clip()
						ctx.translate(10.0, sheetY + sheetH * 0.5)
						ctx.rotate(sheetRot * Math.PI / 180)
						ctx.translate(-10.0, -(sheetY + sheetH * 0.5))
						drawRoundedRect(ctx, sheetX, sheetY, sheetW, sheetH, 1.4)
						ctx.fillStyle = iconButton.glyphRgba(folderRoot.maskColor, sheetAlpha)
						ctx.fill()
						ctx.fillStyle = iconButton.glyphRgba(folderRoot.tintColor, 0.16 * sheetAlpha * ovClamped)
						ctx.fill()
						ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, sheetAlpha)
						ctx.lineWidth = 1.45
						ctx.stroke()
						// Two crisp staggered document lines inside the sheet.
						ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, 0.78 * sheetAlpha)
						ctx.lineWidth = 1.25
						ctx.beginPath()
						ctx.moveTo(sheetX + 2.0, sheetY + 2.3)
						ctx.lineTo(sheetX + sheetW - 2.0, sheetY + 2.3)
						ctx.stroke()
						ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, 0.56 * sheetAlpha)
						ctx.beginPath()
						ctx.moveTo(sheetX + 2.0, sheetY + 4.1)
						ctx.lineTo(sheetX + sheetW - 3.6, sheetY + 4.1)
						ctx.stroke()
						ctx.restore()
					}

					// Layer 3 (always front): bottom-hinged perspective front flap physically occluding lower half of sheet.
					if (ov > 0.005) {
						const flapTopAlpha = iconButton.smoothstep(0.0, 0.10, ov)
						const flapFillAlpha = iconButton.smoothstep(0.10, 0.24, ov)
						const flare = 1.15 * Math.max(0.0, ov - 0.10) / 0.90
						const topY = 8.2 + (10.4 - 8.2) * ov
						const tlX = 3.0 - flare
						const trX = 17.0 + flare
						const flR = 1.5
						const fbR = 1.8
						if (ov <= 0.10) {
							// Fade only the top flap edge while aligned with Layer 1 so the outer contour is never thinned.
							ctx.beginPath()
							ctx.moveTo(3.0, topY + flR)
							ctx.arcTo(3.0, topY, 3.0 + flR, topY, flR)
							ctx.lineTo(17.0 - flR, topY)
							ctx.arcTo(17.0, topY, 17.0, topY + flR, flR)
							ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, flapTopAlpha)
							ctx.lineWidth = 1.75
							ctx.stroke()
						} else {
							ctx.beginPath()
							ctx.moveTo(tlX + flR, topY)
							ctx.lineTo(trX - flR, topY)
							ctx.arcTo(trX, topY, 17.0, 16.6, flR)
							ctx.arcTo(17.0, 16.6, 3.0, 16.6, fbR)
							ctx.arcTo(3.0, 16.6, tlX, topY, fbR)
							ctx.arcTo(tlX, topY, trX, topY, flR)
							ctx.closePath()
							if (flapFillAlpha > 0.005) {
								ctx.fillStyle = iconButton.glyphRgba(folderRoot.maskColor, flapFillAlpha)
								ctx.fill()
								ctx.fillStyle = iconButton.glyphRgba(folderRoot.tintColor, 0.10 * flapFillAlpha * ovClamped)
								ctx.fill()
							}
							ctx.strokeStyle = iconButton.glyphRgba(folderRoot.strokeColor, 1.0)
							ctx.lineWidth = 1.75
							ctx.stroke()
						}
					}
				}
			}
		}
	}

	// 11. Trash: HiDPI Canvas 1:1 reproduction of assets/icons/trash-16.svg with hinged hollow-handle lid.
	Component {
		id: trashGlyph

		Item {
			id: trashRoot
			anchors.fill: parent
			readonly property real pad: 4
			property real lidOpen: iconButton.pressed ? 0.35 : (iconButton.hovered ? 1.0 : 0.0)
			property color strokeColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.dangerText : iconButton.iconColor

			Behavior on lidOpen { SpringAnimation { spring: 5.0; damping: 0.26; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onLidOpenChanged: trashCanvas.requestPaint()
			onStrokeColorChanged: trashCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { trashCanvas.requestPaint() }
			}

			Canvas {
				id: trashCanvas
				x: -trashRoot.pad
				y: -trashRoot.pad
				readonly property real logicalW: trashRoot.width + 2 * trashRoot.pad
				readonly property real logicalH: trashRoot.height + 2 * trashRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || trashRoot.width <= 0 || trashRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const s = trashRoot.width / 16.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(trashRoot.pad, trashRoot.pad)
					ctx.scale(s, s)

					ctx.strokeStyle = iconButton.glyphRgba(trashRoot.strokeColor, 1.0)
					ctx.lineWidth = 1.35
					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					// 1. Open-top bin body & two slender interior ribs (trash-16.svg proportions).
					ctx.beginPath()
					ctx.moveTo(4.25, 4.25)
					ctx.lineTo(4.25, 12.25)
					ctx.arcTo(4.25, 13.75, 5.75, 13.75, 1.5)
					ctx.lineTo(10.25, 13.75)
					ctx.arcTo(11.75, 13.75, 11.75, 12.25, 1.5)
					ctx.lineTo(11.75, 4.25)
					ctx.moveTo(6.75, 7.25)
					ctx.lineTo(6.75, 10.75)
					ctx.moveTo(9.25, 7.25)
					ctx.lineTo(9.25, 10.75)
					ctx.stroke()

					// 1b. Subtle top rim closing the open bin mouth cleanly as the lid lifts away.
					const lo = trashRoot.lidOpen
					const rimAlpha = Math.max(0, Math.min(1, lo)) * 0.55
					if (rimAlpha > 0.01) {
						ctx.strokeStyle = iconButton.glyphRgba(trashRoot.strokeColor, rimAlpha)
						ctx.lineWidth = 1.2
						ctx.beginPath()
						ctx.moveTo(4.25, 4.25)
						ctx.lineTo(11.75, 4.25)
						ctx.stroke()
					}

					// 2. Left-hinged lid bar + crisp hollow arched handle (x: 6..10, y: 2.0..4.25) pivoting around (3.25, 4.25).
					ctx.strokeStyle = iconButton.glyphRgba(trashRoot.strokeColor, 1.0)
					ctx.lineWidth = 1.35
					ctx.save()
					ctx.translate(3.25, 4.25 - 0.7 * lo)
					ctx.rotate((-24.0 * lo) * Math.PI / 180)
					ctx.translate(-3.25, -4.25)
					ctx.beginPath()
					ctx.moveTo(3.25, 4.25)
					ctx.lineTo(12.75, 4.25)
					ctx.moveTo(6.0, 4.25)
					ctx.lineTo(6.0, 2.8)
					ctx.arcTo(6.0, 2.0, 6.8, 2.0, 0.8)
					ctx.lineTo(9.2, 2.0)
					ctx.arcTo(10.0, 2.0, 10.0, 2.8, 0.8)
					ctx.lineTo(10.0, 4.25)
					ctx.stroke()
					ctx.restore()
				}
			}
		}
	}

	// 12. Stash / Stash-Count: HiDPI Canvas 1:1 reproduction of stash-20.svg & stash-count-20.svg with a strict 3-layer sandwich:
	// Layer 1 (back trapezoidal rim) -> Layer 2 (session card or centered count card lifting from pocket) -> Layer 3 (front wall).
	Component {
		id: stashGlyph

		Item {
			id: stashRoot
			anchors.fill: parent
			readonly property real pad: 4
			readonly property string badge: iconButton.badgeText
			property real lift: iconButton.pressed ? 0.3 : (iconButton.hovered ? 1.0 : 0.0)
			property color strokeColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.infoText : iconButton.iconColor
			property color badgeColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.infoText : Theme.text
			property color maskColor: iconButton.color
			property color tintColor: Theme.info

			Behavior on lift { SpringAnimation { spring: 4.8; damping: 0.30; epsilon: 0.004 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on badgeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onBadgeChanged: stashCanvas.requestPaint()
			onLiftChanged: stashCanvas.requestPaint()
			onStrokeColorChanged: stashCanvas.requestPaint()
			onBadgeColorChanged: stashCanvas.requestPaint()
			onMaskColorChanged: stashCanvas.requestPaint()
			onTintColorChanged: stashCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { stashCanvas.requestPaint() }
			}

			Canvas {
				id: stashCanvas
				x: -stashRoot.pad
				y: -stashRoot.pad
				readonly property real logicalW: stashRoot.width + 2 * stashRoot.pad
				readonly property real logicalH: stashRoot.height + 2 * stashRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				function drawRoundedRect(ctx, x, y, w, h, r) {
					const rr = Math.min(r, w * 0.5, h * 0.5)
					ctx.beginPath()
					ctx.moveTo(x + rr, y)
					ctx.lineTo(x + w - rr, y)
					ctx.arcTo(x + w, y, x + w, y + rr, rr)
					ctx.lineTo(x + w, y + h - rr)
					ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr)
					ctx.lineTo(x + rr, y + h)
					ctx.arcTo(x, y + h, x, y + h - rr, rr)
					ctx.lineTo(x, y + rr)
					ctx.arcTo(x, y, x + rr, y, rr)
					ctx.closePath()
				}

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || stashRoot.width <= 0 || stashRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const u = stashRoot.width / 20.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(stashRoot.pad, stashRoot.pad)
					ctx.scale(u, u)

					ctx.lineCap = "round"
					ctx.lineJoin = "round"
					const lv = Math.max(0, Math.min(1.25, stashRoot.lift))
					const badge = stashRoot.badge
					const hasBadge = badge.length > 0

					// Layer 1 (always back): trapezoidal upper tray rim (exact stash-20.svg / stash-count-20.svg).
					ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, 1.0)
					ctx.lineWidth = 1.75
					ctx.beginPath()
					ctx.moveTo(3.0, 11.0)
					ctx.lineTo(3.0, 7.0)
					ctx.lineTo(5.0, 3.0)
					ctx.lineTo(15.0, 3.0)
					ctx.lineTo(17.0, 7.0)
					ctx.lineTo(17.0, 11.0)
					ctx.stroke()

					if (!hasBadge) {
						// Empty stash (0 groups): exact 1:1 stash-20.svg 3-layer sandwich.
						const cardAlpha = iconButton.smoothstep(0.0, 0.15, lv)
						if (cardAlpha > 0.01) {
							const cardW = 8.6
							const cardH = 6.4
							const cardX = (20.0 - cardW) * 0.5
							const cardY = 11.2 + (4.2 - 11.2) * lv
							const cardRot = -2.0 * lv
							ctx.save()
							ctx.translate(10.0, cardY + cardH * 0.5)
							ctx.rotate(cardRot * Math.PI / 180)
							ctx.translate(-10.0, -(cardY + cardH * 0.5))
							drawRoundedRect(ctx, cardX, cardY, cardW, cardH, 1.4)
							ctx.fillStyle = iconButton.glyphRgba(stashRoot.maskColor, cardAlpha)
							ctx.fill()
							ctx.fillStyle = iconButton.glyphRgba(stashRoot.tintColor, 0.14 * cardAlpha * Math.min(1.0, lv))
							ctx.fill()
							ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, cardAlpha)
							ctx.lineWidth = 1.45
							ctx.stroke()
							ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, 0.72 * cardAlpha)
							ctx.lineWidth = 1.25
							ctx.beginPath()
							ctx.moveTo(cardX + 2.0, cardY + 2.5)
							ctx.lineTo(cardX + cardW - 2.0, cardY + 2.5)
							ctx.stroke()
							ctx.restore()
						}

						// Layer 3 (always front): opaque U-notched front wall physically masking the lower half of the card.
						ctx.beginPath()
						ctx.moveTo(3.0, 11.0)
						ctx.lineTo(7.0, 11.0)
						ctx.lineTo(8.0, 13.0)
						ctx.lineTo(12.0, 13.0)
						ctx.lineTo(13.0, 11.0)
						ctx.lineTo(17.0, 11.0)
						ctx.lineTo(17.0, 15.0)
						ctx.arcTo(17.0, 17.0, 15.0, 17.0, 2.0)
						ctx.lineTo(5.0, 17.0)
						ctx.arcTo(3.0, 17.0, 3.0, 15.0, 2.0)
						ctx.closePath()
						ctx.fillStyle = iconButton.glyphRgba(stashRoot.maskColor, 1.0)
						ctx.fill()
						ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, 1.0)
						ctx.lineWidth = 1.75
						ctx.stroke()
					} else {
						// Stash-Count (1..9+ groups): clean centered count at rest; lifts on a sandwiched card behind the emerging U-notched front wall on hover.
						const isWide = badge.length > 1
						const cardAlpha = iconButton.smoothstep(0.08, 0.42, lv)
						const wallT = iconButton.smoothstep(0.06, 0.50, lv)
						const cardW = isWide ? 11.2 : 9.6
						const cardH = 9.8
						const cardX = (20.0 - cardW) * 0.5
						const cardY = 6.0 + (2.2 - 6.0) * lv
						const cardRot = -2.0 * lv
						const numY = cardY + (4.8 + 0.55 * lv) + (isWide ? 0.0 : 0.15)

						ctx.save()
						ctx.translate(10.0, cardY + cardH * 0.5)
						ctx.rotate(cardRot * Math.PI / 180)
						ctx.translate(-10.0, -(cardY + cardH * 0.5))
						if (cardAlpha > 0.01) {
							drawRoundedRect(ctx, cardX, cardY, cardW, cardH, 1.6)
							ctx.fillStyle = iconButton.glyphRgba(stashRoot.maskColor, cardAlpha)
							ctx.fill()
							ctx.fillStyle = iconButton.glyphRgba(stashRoot.tintColor, 0.14 * cardAlpha * Math.min(1.0, lv))
							ctx.fill()
							ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, cardAlpha)
							ctx.lineWidth = 1.45
							ctx.stroke()
						}
						ctx.fillStyle = iconButton.glyphRgba(stashRoot.badgeColor, 1.0)
						ctx.font = "700 " + (isWide ? "7.2" : "8.4") + "px sans-serif"
						ctx.textAlign = "center"
						ctx.textBaseline = "middle"
						ctx.fillText(badge, 10.0, numY)
						ctx.restore()

						// Layer 3 (front): emerging opaque U-notched pocket wall occluding lower edge of rising numbered card.
						if (wallT > 0.01) {
							ctx.beginPath()
							ctx.moveTo(3.0, 11.0)
							ctx.lineTo(7.0, 11.0)
							ctx.lineTo(8.0, 13.0)
							ctx.lineTo(12.0, 13.0)
							ctx.lineTo(13.0, 11.0)
							ctx.lineTo(17.0, 11.0)
							ctx.lineTo(17.0, 15.0)
							ctx.arcTo(17.0, 17.0, 15.0, 17.0, 2.0)
							ctx.lineTo(5.0, 17.0)
							ctx.arcTo(3.0, 17.0, 3.0, 15.0, 2.0)
							ctx.closePath()
							ctx.fillStyle = iconButton.glyphRgba(stashRoot.maskColor, wallT)
							ctx.fill()
							ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, wallT)
							ctx.lineWidth = 1.75
							ctx.beginPath()
							ctx.moveTo(3.0, 11.0)
							ctx.lineTo(7.0, 11.0)
							ctx.lineTo(8.0, 13.0)
							ctx.lineTo(12.0, 13.0)
							ctx.lineTo(13.0, 11.0)
							ctx.lineTo(17.0, 11.0)
							ctx.stroke()
						}
						// Crisp outer lower box contour always stays rock-solid at alpha = 1.0 (exact stash-count-20.svg).
						ctx.strokeStyle = iconButton.glyphRgba(stashRoot.strokeColor, 1.0)
						ctx.lineWidth = 1.75
						ctx.beginPath()
						ctx.moveTo(3.0, 11.0)
						ctx.lineTo(3.0, 15.0)
						ctx.arcTo(3.0, 17.0, 5.0, 17.0, 2.0)
						ctx.lineTo(15.0, 17.0)
						ctx.arcTo(17.0, 17.0, 17.0, 15.0, 2.0)
						ctx.lineTo(17.0, 11.0)
						ctx.stroke()
					}
				}
			}
		}
	}

	// 13. Bell / Bell-Off: persistent HiDPI Canvas 1:1 bell-20.svg with damped sine ring & cutout mute slash.
	Component {
		id: bellGlyph

		Item {
			id: bellRoot
			anchors.fill: parent
			readonly property real pad: 4
			readonly property bool isMuted: iconButton.iconName === "bell-off"
			property real muteProgress: isMuted ? 1.0 : 0.0
			property real hoverVal: iconButton.pressed ? -0.2 : (iconButton.hovered ? 1.0 : 0.0)
			property real ringPhase: 1.0
			property bool bellReady: false
			property color strokeColor: !isMuted && iconButton.isMutedDefault && (iconButton.hovered || ringPhase < 1.0)
				? Theme.warningText
				: iconButton.iconColor
			property color waveColor: Theme.warningText
			property color maskColor: iconButton.color

			Behavior on muteProgress { NumberAnimation { duration: 280; easing.type: Easing.InOutCubic } }
			Behavior on hoverVal { SpringAnimation { spring: 5.0; damping: 0.30; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			function triggerRing() {
				if (isMuted)
					return
				ringAnim.restart()
			}

			Component.onCompleted: {
				bellReady = true
				bellCanvas.requestPaint()
			}

			onIsMutedChanged: {
				if (!bellReady)
					return
				if (isMuted) {
					ringAnim.stop()
					ringPhase = 1.0
				} else {
					ringAnim.restart()
				}
			}

			NumberAnimation {
				id: ringAnim
				target: bellRoot
				property: "ringPhase"
				from: 0.0
				to: 1.0
				duration: 680
				easing.type: Easing.Linear
			}

			onRingPhaseChanged: bellCanvas.requestPaint()
			onMuteProgressChanged: bellCanvas.requestPaint()
			onHoverValChanged: bellCanvas.requestPaint()
			onStrokeColorChanged: bellCanvas.requestPaint()
			onWaveColorChanged: bellCanvas.requestPaint()
			onMaskColorChanged: bellCanvas.requestPaint()

			Connections {
				target: iconButton
				function onHoveredChanged() { if (iconButton.hovered && !bellRoot.isMuted) bellRoot.triggerRing() }
				function onClicked() { if (!bellRoot.isMuted) bellRoot.triggerRing() }
				function onGlyphPixelScaleChanged() { bellCanvas.requestPaint() }
			}

			Canvas {
				id: bellCanvas
				x: -bellRoot.pad
				y: -bellRoot.pad
				readonly property real logicalW: bellRoot.width + 2 * bellRoot.pad
				readonly property real logicalH: bellRoot.height + 2 * bellRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || bellRoot.width <= 0 || bellRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const u = bellRoot.width / 20.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(bellRoot.pad, bellRoot.pad)
					ctx.scale(u, u)

					const mp = Math.max(0, Math.min(1, bellRoot.muteProgress))
					const hv = Math.max(0, bellRoot.hoverVal)
					const t = Math.max(0, Math.min(1, bellRoot.ringPhase))
					const activeRing = t < 1.0 && mp < 0.99
					const ringWeight = 1.0 - mp
					const swingDeg = activeRing ? Math.sin(t * Math.PI * 3.0) * Math.pow(1.0 - t, 1.5) * 20.0 * ringWeight : 0.0
					const clapperShift = activeRing ? Math.sin(Math.max(0, t - 0.06) * Math.PI * 3.0) * Math.pow(1.0 - t, 1.4) * 2.1 * ringWeight : 0.0
					const bellAlpha = Math.max(0, Math.min(1, 1.0 - 0.35 * mp + 0.15 * mp * hv))
					const strokeW = 1.85

					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					// 1. Subtle warm amber soundwave arcs flanking the ringing bell (fade away when muted).
					if (activeRing) {
						const waveNorm = Math.max(0, Math.min(1, (t - 0.04) / 0.76))
						const waveAlpha = Math.sin(Math.PI * waveNorm) * 0.82 * ringWeight
						if (waveAlpha > 0.01) {
							const waveR = 7.2 + (1.0 - Math.pow(1.0 - waveNorm, 2)) * 1.4
							ctx.strokeStyle = iconButton.glyphRgba(bellRoot.waveColor, waveAlpha)
							ctx.lineWidth = 1.35
							ctx.beginPath()
							ctx.arc(10.0, 9.0, waveR, Math.PI * 0.86, Math.PI * 1.14, false)
							ctx.stroke()
							ctx.beginPath()
							ctx.arc(10.0, 9.0, waveR, -Math.PI * 0.14, Math.PI * 0.14, false)
							ctx.stroke()
						}
					}

					ctx.strokeStyle = iconButton.glyphRgba(bellRoot.strokeColor, bellAlpha)
					ctx.lineWidth = strokeW

					// 2. Clapper with inertial horizontal lag.
					ctx.beginPath()
					ctx.moveTo(8.0 + clapperShift, 17.0)
					ctx.quadraticCurveTo(10.0 + clapperShift, 17.45, 12.0 + clapperShift, 17.0)
					ctx.stroke()

					// 3. Bell dome & brim pivoting cleanly around top crown (10, 4).
					ctx.save()
					ctx.translate(10.0, 4.0)
					ctx.rotate(swingDeg * Math.PI / 180)
					ctx.translate(-10.0, -4.0)
					ctx.beginPath()
					ctx.moveTo(5.0, 14.0)
					ctx.lineTo(5.0, 9.0)
					ctx.arc(10.0, 9.0, 5.0, Math.PI, 0, false)
					ctx.lineTo(15.0, 14.0)
					ctx.moveTo(3.0, 14.0)
					ctx.lineTo(17.0, 14.0)
					ctx.stroke()
					ctx.restore()

					// 4. Cutout diagonal mute slash (retracts 18% on hover when muted to anticipate unmuting).
					if (mp > 0.01) {
						const slashFrac = mp * (1.0 - 0.18 * mp * hv)
						const sx0 = 3.6
						const sy0 = 3.6
						const sx1 = sx0 + (16.4 - sx0) * slashFrac
						const sy1 = sy0 + (16.4 - sy0) * slashFrac
						// Background cutout pass separating slash from bell silhouette.
						ctx.strokeStyle = iconButton.glyphRgba(bellRoot.maskColor, Math.min(1.0, mp * 1.4))
						ctx.lineWidth = strokeW + 2.2
						ctx.beginPath()
						ctx.moveTo(sx0, sy0)
						ctx.lineTo(sx1, sy1)
						ctx.stroke()
						// Foreground rounded diagonal slash.
						ctx.strokeStyle = iconButton.glyphRgba(bellRoot.strokeColor, Math.min(1.0, mp * 1.25))
						ctx.lineWidth = strokeW
						ctx.beginPath()
						ctx.moveTo(sx0, sy0)
						ctx.lineTo(sx1, sy1)
						ctx.stroke()
					}
				}
			}
		}
	}

	// 14. Sliders: HiDPI Canvas 1:1 reproduction of assets/icons/sliders-20.svg with 3 staggered spring faders.
	Component {
		id: slidersGlyph

		Item {
			id: slidersRoot
			anchors.fill: parent
			readonly property real pad: 3
			property real cx1: iconButton.pressed ? 10.5 : (iconButton.hovered ? 13.0 : 7.0)
			property real cx2: iconButton.pressed ? 9.5 : (iconButton.hovered ? 7.0 : 13.0)
			property real cx3: iconButton.pressed ? 10.0 : (iconButton.hovered ? 12.0 : 7.0)
			property color railColor: iconButton.iconColor
			property color knobColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.accentText : iconButton.iconColor

			Behavior on cx1 { SpringAnimation { spring: 5.2; damping: 0.28; epsilon: 0.02 } }
			Behavior on cx2 { SpringAnimation { spring: 4.4; damping: 0.26; epsilon: 0.02 } }
			Behavior on cx3 { SpringAnimation { spring: 3.9; damping: 0.27; epsilon: 0.02 } }
			Behavior on railColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on knobColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onCx1Changed: slidersCanvas.requestPaint()
			onCx2Changed: slidersCanvas.requestPaint()
			onCx3Changed: slidersCanvas.requestPaint()
			onRailColorChanged: slidersCanvas.requestPaint()
			onKnobColorChanged: slidersCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { slidersCanvas.requestPaint() }
			}

			Canvas {
				id: slidersCanvas
				x: -slidersRoot.pad
				y: -slidersRoot.pad
				readonly property real logicalW: slidersRoot.width + 2 * slidersRoot.pad
				readonly property real logicalH: slidersRoot.height + 2 * slidersRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || slidersRoot.width <= 0 || slidersRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const u = slidersRoot.width / 20.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(slidersRoot.pad, slidersRoot.pad)
					ctx.scale(u, u)

					ctx.lineWidth = 1.75
					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					const r = 2.0
					const gap = r + 0.1
					const rows = [
						{ y: 5.0, cx: slidersRoot.cx1 },
						{ y: 10.0, cx: slidersRoot.cx2 },
						{ y: 15.0, cx: slidersRoot.cx3 }
					]

					// 1. Dynamic split rails cleanly broken around each circular slider knob.
					ctx.strokeStyle = iconButton.glyphRgba(slidersRoot.railColor, 1.0)
					ctx.beginPath()
					for (let i = 0; i < rows.length; ++i) {
						const y = rows[i].y
						const cx = rows[i].cx
						if (cx - gap > 3.0) {
							ctx.moveTo(3.0, y)
							ctx.lineTo(cx - gap, y)
						}
						if (cx + gap < 17.0) {
							ctx.moveTo(cx + gap, y)
							ctx.lineTo(17.0, y)
						}
					}
					ctx.stroke()

					// 2. Three circular slider knobs brightening to Theme.accentText on hover.
					ctx.strokeStyle = iconButton.glyphRgba(slidersRoot.knobColor, 1.0)
					for (let i = 0; i < rows.length; ++i) {
						ctx.beginPath()
						ctx.arc(rows[i].cx, rows[i].y, r, 0, Math.PI * 2)
						ctx.stroke()
					}
				}
			}
		}
	}

	// 15. File-Text: HiDPI Canvas 1:1 reproduction of assets/icons/file-text-16.svg with subtle page lift, fold flex & staggered config lines.
	Component {
		id: fileTextGlyph

		Item {
			id: fileTextRoot
			anchors.fill: parent
			readonly property real pad: 3
			property real hoverVal: iconButton.pressed ? -0.25 : (iconButton.hovered ? 1.0 : 0.0)
			property real line1Val: iconButton.hovered ? 1.0 : 0.0
			property real line2Val: iconButton.hovered ? 1.0 : 0.0
			property color strokeColor: iconButton.iconColor
			property color lineColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.infoText : iconButton.iconColor

			Behavior on hoverVal { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.005 } }
			Behavior on line1Val { SpringAnimation { spring: 5.2; damping: 0.28; epsilon: 0.005 } }
			Behavior on line2Val { SpringAnimation { spring: 4.0; damping: 0.26; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on lineColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onHoverValChanged: fileTextCanvas.requestPaint()
			onLine1ValChanged: fileTextCanvas.requestPaint()
			onLine2ValChanged: fileTextCanvas.requestPaint()
			onStrokeColorChanged: fileTextCanvas.requestPaint()
			onLineColorChanged: fileTextCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { fileTextCanvas.requestPaint() }
			}

			Canvas {
				id: fileTextCanvas
				x: -fileTextRoot.pad
				y: -fileTextRoot.pad
				readonly property real logicalW: fileTextRoot.width + 2 * fileTextRoot.pad
				readonly property real logicalH: fileTextRoot.height + 2 * fileTextRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || fileTextRoot.width <= 0 || fileTextRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const s = fileTextRoot.width / 16.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(fileTextRoot.pad, fileTextRoot.pad)
					ctx.scale(s, s)

					const h = fileTextRoot.hoverVal
					const l1 = fileTextRoot.line1Val
					const l2 = fileTextRoot.line2Val

					ctx.lineWidth = 1.35
					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					ctx.save()
					ctx.translate(8.0, 8.0 - 0.6 * h)
					ctx.rotate((-3.5 * h) * Math.PI / 180)
					ctx.translate(-8.0, -8.0)

					// 1. Folded-corner document outline with crisp open top-right bevel.
					ctx.strokeStyle = iconButton.glyphRgba(fileTextRoot.strokeColor, 1.0)
					ctx.beginPath()
					ctx.moveTo(3.25, 12.25)
					ctx.lineTo(3.25, 3.75)
					ctx.arcTo(3.25, 2.25, 4.75, 2.25, 1.5)
					ctx.lineTo(9.0, 2.25)
					ctx.lineTo(12.75, 6.0)
					ctx.lineTo(12.75, 12.25)
					ctx.arcTo(12.75, 13.75, 11.25, 13.75, 1.5)
					ctx.lineTo(4.75, 13.75)
					ctx.arcTo(3.25, 13.75, 3.25, 12.25, 1.5)
					ctx.closePath()
					ctx.stroke()

					// 2. Crisp right-angle dog-ear fold with clean breathing space inside the triangle.
					ctx.beginPath()
					ctx.moveTo(9.0, 2.25)
					ctx.lineTo(9.0, 6.0)
					ctx.lineTo(12.75, 6.0)
					ctx.stroke()
					// 3. Two staggered config text lines breathing and lighting up in Theme.infoText.
					ctx.strokeStyle = iconButton.glyphRgba(fileTextRoot.lineColor, 1.0)
					ctx.beginPath()
					ctx.moveTo(5.75 + (5.4 - 5.75) * l1, 8.25)
					ctx.lineTo(10.25 + (10.6 - 10.25) * l1, 8.25)
					ctx.moveTo(5.75 + (5.4 - 5.75) * l2, 10.75)
					ctx.lineTo(10.25 + (9.1 - 10.25) * l2, 10.75)
					ctx.stroke()
					ctx.restore()
				}
			}
		}
	}

	// 16. Chat-List ("Move to background"): HiDPI Canvas 1:1 reproduction of assets/icons/chat-list-16.svg.
	// Hover tucks the active chat bubble back into the background while the session list card rises forward.
	Component {
		id: chatListGlyph

		Item {
			id: chatListRoot
			anchors.fill: parent
			readonly property real pad: 3
			property real hoverVal: iconButton.pressed ? 0.55 : (iconButton.hovered ? 1.0 : 0.0)
			property real line1Val: iconButton.hovered ? 1.0 : 0.0
			property real line2Val: iconButton.hovered ? 1.0 : 0.0
			property color strokeColor: iconButton.iconColor
			property color frontColor: iconButton.hovered && iconButton.isMutedDefault ? Theme.accentText : iconButton.iconColor

			Behavior on hoverVal { SpringAnimation { spring: 4.8; damping: 0.28; epsilon: 0.005 } }
			Behavior on line1Val { SpringAnimation { spring: 5.4; damping: 0.28; epsilon: 0.005 } }
			Behavior on line2Val { SpringAnimation { spring: 4.2; damping: 0.26; epsilon: 0.005 } }
			Behavior on strokeColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
			Behavior on frontColor { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			onHoverValChanged: chatListCanvas.requestPaint()
			onLine1ValChanged: chatListCanvas.requestPaint()
			onLine2ValChanged: chatListCanvas.requestPaint()
			onStrokeColorChanged: chatListCanvas.requestPaint()
			onFrontColorChanged: chatListCanvas.requestPaint()

			Connections {
				target: iconButton
				function onGlyphPixelScaleChanged() { chatListCanvas.requestPaint() }
				function onColorChanged() { chatListCanvas.requestPaint() }
			}

			Canvas {
				id: chatListCanvas
				x: -chatListRoot.pad
				y: -chatListRoot.pad
				readonly property real logicalW: chatListRoot.width + 2 * chatListRoot.pad
				readonly property real logicalH: chatListRoot.height + 2 * chatListRoot.pad
				width: Math.max(1, Math.ceil(logicalW * iconButton.glyphPixelScale))
				height: Math.max(1, Math.ceil(logicalH * iconButton.glyphPixelScale))
				transformOrigin: Item.TopLeft
				scale: 1 / iconButton.glyphPixelScale
				renderTarget: Canvas.Image

				onWidthChanged: requestPaint()
				onHeightChanged: requestPaint()
				Component.onCompleted: requestPaint()

				onPaint: {
					const ctx = getContext("2d")
					if (!ctx || chatListRoot.width <= 0 || chatListRoot.height <= 0)
						return
					ctx.setTransform(1, 0, 0, 1, 0, 0)
					ctx.clearRect(0, 0, width, height)
					const ps = iconButton.glyphPixelScale
					const s = chatListRoot.width / 16.0
					ctx.setTransform(ps, 0, 0, ps, 0, 0)
					ctx.translate(chatListRoot.pad, chatListRoot.pad)
					ctx.scale(s, s)

					const h = chatListRoot.hoverVal
					const hClamped = Math.max(0, Math.min(1, h))
					const l1 = Math.max(0, Math.min(1.2, chatListRoot.line1Val))
					const l2 = Math.max(0, Math.min(1.2, chatListRoot.line2Val))

					ctx.lineWidth = 1.35
					ctx.lineCap = "round"
					ctx.lineJoin = "round"

					// 1. Active chat bubble (exact chat-list-16.svg) retreating smoothly into the background.
					ctx.save()
					const bubbleSc = 1.0 - 0.16 * h
					ctx.translate(8.0 + 1.65 * h, 8.0 - 1.35 * h)
					ctx.rotate((4.0 * h) * Math.PI / 180)
					ctx.scale(bubbleSc, bubbleSc)
					ctx.translate(-8.0, -8.0)
					const bubbleAlpha = Math.max(0.25, Math.min(1.0, 1.0 - 0.45 * hClamped))
					ctx.strokeStyle = iconButton.glyphRgba(chatListRoot.strokeColor, bubbleAlpha)
					ctx.beginPath()
					ctx.moveTo(3.75, 2.25)
					ctx.lineTo(12.25, 2.25)
					ctx.arcTo(13.75, 2.25, 13.75, 3.75, 1.5)
					ctx.lineTo(13.75, 9.75)
					ctx.arcTo(13.75, 11.25, 12.25, 11.25, 1.5)
					ctx.lineTo(8.25, 11.25)
					ctx.lineTo(5.75, 13.75)
					ctx.lineTo(5.75, 11.25)
					ctx.lineTo(3.75, 11.25)
					ctx.arcTo(2.25, 11.25, 2.25, 9.75, 1.5)
					ctx.lineTo(2.25, 3.75)
					ctx.arcTo(2.25, 2.25, 3.75, 2.25, 1.5)
					ctx.closePath()
					ctx.stroke()
					const innerAlpha = Math.max(0, Math.min(1.0, 1.0 - 0.70 * hClamped))
					if (innerAlpha > 0.02) {
						ctx.strokeStyle = iconButton.glyphRgba(chatListRoot.strokeColor, innerAlpha)
						ctx.beginPath()
						ctx.moveTo(5.75, 5.25)
						ctx.lineTo(10.25, 5.25)
						ctx.moveTo(5.75, 8.25)
						ctx.lineTo(8.25, 8.25)
						ctx.stroke()
					}
					ctx.restore()

					// 2. Foreground background-session list card rising forward with clean surface cutout.
					if (hClamped > 0.01) {
						ctx.save()
						const cardSc = 0.86 + 0.14 * h
						ctx.translate(6.75 - 1.4 * (1 - h), 10.0 + 1.4 * (1 - h))
						ctx.scale(cardSc, cardSc)
						ctx.translate(-6.75, -10.0)
						ctx.beginPath()
						ctx.moveTo(4.05, 6.25)
						ctx.lineTo(9.45, 6.25)
						ctx.arcTo(11.25, 6.25, 11.25, 8.05, 1.8)
						ctx.lineTo(11.25, 11.95)
						ctx.arcTo(11.25, 13.75, 9.45, 13.75, 1.8)
						ctx.lineTo(4.05, 13.75)
						ctx.arcTo(2.25, 13.75, 2.25, 11.95, 1.8)
						ctx.lineTo(2.25, 8.05)
						ctx.arcTo(2.25, 6.25, 4.05, 6.25, 1.8)
						ctx.closePath()
						ctx.fillStyle = iconButton.glyphRgba(iconButton.color, hClamped)
						ctx.fill()
						ctx.strokeStyle = iconButton.glyphRgba(chatListRoot.frontColor, hClamped)
						ctx.stroke()
						// Two staggered session rows (status dot + bar) inside the list card.
						ctx.fillStyle = iconButton.glyphRgba(chatListRoot.frontColor, hClamped)
						ctx.beginPath()
						ctx.arc(4.45, 8.75, 0.65, 0, Math.PI * 2)
						ctx.arc(4.45, 11.25, 0.65, 0, Math.PI * 2)
						ctx.fill()
						ctx.lineWidth = 1.25
						ctx.beginPath()
						ctx.moveTo(6.35, 8.75)
						ctx.lineTo(6.35 + 2.95 * l1, 8.75)
						ctx.moveTo(6.35, 11.25)
						ctx.lineTo(6.35 + 2.15 * l2, 11.25)
						ctx.stroke()
						ctx.restore()
					}
				}
			}
		}
	}
}
