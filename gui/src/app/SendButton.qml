pragma ComponentBehavior: Bound
// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// Round send button that morphs into stop while a reply is running.
// All state transitions and hover/press anticipations use interruptible reactive bindings
// and SpringAnimations so send <-> stop morphs and pointer gestures stay fluid at any moment.
// Only the face scales and tracks the cursor; the handlers stay on the unscaled root (see CircleButton).
Item {
	id: button

	property bool busy: false
	property bool canSend: false
	property color activeColor: Theme.accent
	signal clicked()

	readonly property bool active: busy || canSend
	readonly property bool activeHovered: active && hover.hovered
	readonly property bool activePressed: active && tap.pressed
	readonly property real hiddenGlyphScale: 0.5 // a glyph shrinks to this as it fades out
	readonly property int upRotation: -90 // of the chevron that points right, to point up; the stop glyph turns in from it

	readonly property color rimColor: busy ? Theme.danger : activeColor
	readonly property color rimCoreColor: busy ? Theme.dangerText : activeColor
	readonly property real rimStrengthScale: busy ? 0.56 : (canSend ? 0.42 : 0.0)
	readonly property real haloPad: 12
	readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)
	readonly property real halfWidth: Math.max(1, width * 0.5)
	readonly property real halfHeight: Math.max(1, height * 0.5)
	readonly property real cursorNx: activeHovered ? Math.max(-1, Math.min(1, (hover.point.position.x - halfWidth) / halfWidth)) : 0
	readonly property real cursorNy: activeHovered ? Math.max(-1, Math.min(1, (hover.point.position.y - halfHeight) / halfHeight)) : 0

	property real targetGlowAngle: -Math.PI * 0.5
	property real glowAngle: targetGlowAngle
	property real glowStrength: activeHovered ? 1.0 : 0.0
	property real pressPulse: activePressed ? 1.0 : 0.0

	width: 32
	height: 32

	Behavior on glowAngle {
		SpringAnimation {
			spring: 4.5
			damping: 0.38
			epsilon: 0.01
		}
	}
	Behavior on glowStrength {
		NumberAnimation {
			duration: Motion.standard
			easing.type: Motion.ease
		}
	}
	Behavior on pressPulse {
		NumberAnimation {
			duration: Motion.quick
			easing.type: Motion.ease
		}
	}

	function cssRgba(c, alpha) {
		const r = Math.round(c.r * 255)
		const g = Math.round(c.g * 255)
		const b = Math.round(c.b * 255)
		const a = Math.max(0, Math.min(1, alpha))
		return "rgba(" + r + "," + g + "," + b + "," + a.toFixed(4) + ")"
	}

	function updateGlowTarget() {
		const mag2 = cursorNx * cursorNx + cursorNy * cursorNy
		if (!activeHovered || mag2 <= 0.01)
			return
		const raw = Math.atan2(cursorNy, cursorNx)
		let delta = raw - targetGlowAngle
		while (delta > Math.PI)
			delta -= 2 * Math.PI
		while (delta < -Math.PI)
			delta += 2 * Math.PI
		targetGlowAngle += delta
	}

	function scheduleRimPaint() {
		if (rimCanvas.visible)
			rimCanvas.requestPaint()
	}

	onActiveChanged: updateGlowTarget()
	onGlowAngleChanged: scheduleRimPaint()
	onGlowStrengthChanged: scheduleRimPaint()
	onPressPulseChanged: scheduleRimPaint()
	onRimColorChanged: scheduleRimPaint()
	onRimCoreColorChanged: scheduleRimPaint()
	onRimStrengthScaleChanged: scheduleRimPaint()
	onPixelScaleChanged: scheduleRimPaint()

	Connections {
		target: Theme
		function onDarkChanged() { button.scheduleRimPaint() }
	}

	Rectangle {
		id: face
		anchors.fill: parent
		radius: width / 2
		color: !button.active ? Theme.surfaceHover
			: (button.busy && button.activeHovered ? Theme.danger : button.activeColor)
		scale: button.activePressed ? 0.88 : (button.activeHovered ? 1.08 : 1.0)

		Behavior on color {
			ColorAnimation {
				duration: button.activeHovered ? Motion.quick : Motion.standard
				easing.type: Motion.ease
			}
		}
		Behavior on scale {
			SpringAnimation {
				spring: 5.2
				damping: 0.30
				epsilon: 0.002
			}
		}

		transform: Translate {
			x: button.activeHovered ? button.cursorNx * 1.2 : 0
			y: button.activeHovered ? button.cursorNy * 1.2 : 0

			Behavior on x {
				SpringAnimation {
					spring: 4.2
					damping: 0.34
					epsilon: 0.05
				}
			}
			Behavior on y {
				SpringAnimation {
					spring: 4.2
					damping: 0.34
					epsilon: 0.05
				}
			}
		}

		// Restrained directional outer rim-light; strictly hidden at rest for zero Canvas overhead.
		Canvas {
			id: rimCanvas
			z: -1
			x: -button.haloPad
			y: -button.haloPad
			readonly property real logicalWidth: face.width + 2 * button.haloPad
			readonly property real logicalHeight: face.height + 2 * button.haloPad
			width: Math.max(1, Math.ceil(logicalWidth * button.pixelScale))
			height: Math.max(1, Math.ceil(logicalHeight * button.pixelScale))
			transformOrigin: Item.TopLeft
			scale: 1 / button.pixelScale
			renderTarget: Canvas.Image
			visible: button.rimStrengthScale > 0 && (button.glowStrength > 0.005 || button.pressPulse > 0.005)

			onVisibleChanged: if (visible) requestPaint()
			onWidthChanged: if (visible) requestPaint()
			onHeightChanged: if (visible) requestPaint()

			onPaint: {
				const ctx = getContext("2d")
				if (!ctx)
					return
				ctx.setTransform(1, 0, 0, 1, 0, 0)
				ctx.clearRect(0, 0, width, height)
				if (!visible || face.width <= 0 || face.height <= 0)
					return

				ctx.setTransform(button.pixelScale, 0, 0, button.pixelScale, 0, 0)

				const cx = button.haloPad + face.width * 0.5
				const cy = button.haloPad + face.height * 0.5
				const r = Math.min(face.width, face.height) * 0.5
				const a = button.glowAngle
				const ca = Math.cos(a)
				const sa = Math.sin(a)
				const strength = Math.max(button.glowStrength, button.pressPulse * 0.85) * button.rimStrengthScale
				const alpha = Math.max(0, Math.min(2.2, strength))
				const pressBoost = 1.0 + button.pressPulse * 0.28

				// 1. Soft directional ambient aura radiating outward along `glowAngle`.
				const auraX = cx + ca * r * 0.42
				const auraY = cy + sa * r * 0.42
				const auraR = Math.min(r + button.haloPad - 0.5, r + button.haloPad * 0.68 * pressBoost)
				const auraAlpha = (Theme.dark ? 0.34 : 0.24) * alpha * pressBoost
				const aura = ctx.createRadialGradient(auraX, auraY, r * 0.35, auraX, auraY, auraR)
				aura.addColorStop(0.0, button.cssRgba(button.rimColor, auraAlpha))
				aura.addColorStop(0.58, button.cssRgba(button.rimColor, auraAlpha * 0.36))
				aura.addColorStop(1.0, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = aura
				ctx.beginPath()
				ctx.arc(auraX, auraY, auraR, 0, Math.PI * 2)
				ctx.fill()

				// 2. Luminous mid-bloom crescent hugging the outer rim edge.
				const midShift = 2.0 + button.pressPulse * 0.8
				const midCx = cx + ca * midShift
				const midCy = cy + sa * midShift
				const midApexX = cx + ca * (r + 1.6)
				const midApexY = cy + sa * (r + 1.6)
				const midGrad = ctx.createRadialGradient(midApexX, midApexY, 0, midApexX, midApexY, r * 1.4)
				midGrad.addColorStop(0.0, button.cssRgba(button.rimColor, 0.72 * alpha * pressBoost))
				midGrad.addColorStop(0.55, button.cssRgba(button.rimColor, 0.28 * alpha))
				midGrad.addColorStop(1.0, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = midGrad
				ctx.beginPath()
				ctx.arc(midCx, midCy, r + 0.9, 0, Math.PI * 2)
				ctx.fill()

				// 3. Specular crescent edge peeking cleanly around `face`.
				const coreShift = 1.45 + button.pressPulse * 0.6
				const coreCx = cx + ca * coreShift
				const coreCy = cy + sa * coreShift
				const coreApexX = cx + ca * (r + 1.1)
				const coreApexY = cy + sa * (r + 1.1)
				const coreGrad = ctx.createRadialGradient(coreApexX, coreApexY, 0, coreApexX, coreApexY, r * 1.15)
				coreGrad.addColorStop(0.0, button.cssRgba(button.rimCoreColor, 0.92 * alpha * pressBoost))
				coreGrad.addColorStop(0.45, button.cssRgba(button.rimColor, 0.50 * alpha))
				coreGrad.addColorStop(0.92, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = coreGrad
				ctx.beginPath()
				ctx.arc(coreCx, coreCy, r + 0.45, 0, Math.PI * 2)
				ctx.fill()
			}
		}

		Item {
			id: glyphHost
			z: 1
			anchors.fill: parent

			transform: Translate {
				x: button.activeHovered ? button.cursorNx * 1.5 : 0
				y: button.activeHovered ? button.cursorNy * 1.5 : 0

				Behavior on x {
					SpringAnimation {
						spring: 3.8
						damping: 0.30
						epsilon: 0.05
					}
				}
				Behavior on y {
					SpringAnimation {
						spring: 3.8
						damping: 0.30
						epsilon: 0.05
					}
				}
			}

			Item {
				id: arrowGroup
				anchors.centerIn: parent
				width: 16
				height: 16
				opacity: button.busy ? 0.0 : 1.0
				scale: button.busy ? button.hiddenGlyphScale : (button.activePressed ? 0.90 : (button.activeHovered ? 1.08 : 1.0))
				rotation: button.busy ? 45 : 0

				Behavior on opacity {
					NumberAnimation {
						duration: button.busy ? Motion.quick : Motion.standard
						easing.type: button.busy ? Motion.exitEase : Motion.ease
					}
				}
				Behavior on scale {
					SpringAnimation {
						spring: 5.2
						damping: 0.28
						epsilon: 0.002
					}
				}
				Behavior on rotation {
					SpringAnimation {
						spring: 4.8
						damping: 0.30
						epsilon: 0.05
					}
				}

				transform: Translate {
					y: button.busy ? -4.0 : (button.activePressed ? 1.0 : (button.activeHovered ? -2.2 : 0.0))

					Behavior on y {
						SpringAnimation {
							spring: 5.0
							damping: 0.28
							epsilon: 0.02
						}
					}
				}

				Chevron {
					id: arrow
					anchors.centerIn: parent
					width: 16
					height: 16
					rotation: button.upRotation
					color: button.active ? Theme.onAccent : Theme.textMuted
					strokeWidth: 2

					Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}

				// Short launch stem aligned to the chevron's rotated tip cap at (8.0, 5.6); extends smoothly on hover.
				Rectangle {
					id: arrowStem
					anchors.horizontalCenter: parent.horizontalCenter
					y: 4.6
					width: 2
					height: button.canSend && !button.busy
						? (button.activePressed ? 8.6 : (button.activeHovered ? 10.6 : 0))
						: 0
					radius: width / 2
					color: arrow.color
					opacity: button.canSend && !button.busy && (button.activeHovered || button.activePressed) ? 1.0 : 0.0

					Behavior on height {
						SpringAnimation {
							spring: 5.2
							damping: 0.28
							epsilon: 0.05
						}
					}
					Behavior on opacity {
						NumberAnimation {
							duration: Motion.quick
							easing.type: Motion.ease
						}
					}
				}
			}

			Rectangle {
				id: stopGlyph
				anchors.centerIn: parent
				width: 10
				height: 10
				radius: button.busy && button.activeHovered ? 1.5 : 2.5
				color: Theme.onAccent
				opacity: button.busy ? 1.0 : 0.0
				scale: !button.busy ? button.hiddenGlyphScale : (button.activePressed ? 0.88 : (button.activeHovered ? 1.12 : 1.0))
				rotation: button.busy ? 0 : button.upRotation

				Behavior on radius {
					SpringAnimation {
						spring: 5.2
						damping: 0.32
						epsilon: 0.02
					}
				}
				Behavior on opacity {
					NumberAnimation {
						duration: button.busy ? Motion.standard : Motion.quick
						easing.type: button.busy ? Motion.ease : Motion.exitEase
					}
				}
				Behavior on scale {
					SpringAnimation {
						spring: 5.2
						damping: 0.28
						epsilon: 0.002
					}
				}
				Behavior on rotation {
					SpringAnimation {
						spring: 4.8
						damping: 0.30
						epsilon: 0.05
					}
				}
			}
		}
	}

	HoverHandler {
		id: hover
		cursorShape: button.active ? Qt.PointingHandCursor : Qt.ArrowCursor
		onHoveredChanged: button.updateGlowTarget()
		onPointChanged: button.updateGlowTarget()
	}
	TapHandler {
		id: tap
		enabled: button.active
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.name: busy ? qsTr("Stop") : qsTr("Send")
	Accessible.onPressAction: if (active) clicked()
}
