pragma ComponentBehavior: Bound
// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// Round surface button; the glyph is its child. Set Accessible.name on each use.
//
// Only the face scales and tracks the cursor. The handlers sit on the unscaled root: a handler
// tests its parent's bounds through the parent's scale, so on a shrunken root a release near the
// edge would land outside and never count as a tap.
Item {
	id: button

	signal clicked()

	readonly property color color: face.color // for glyphs painted in the button's color
	readonly property alias hovered: hover.hovered
	readonly property alias pressed: tap.pressed
	default property alias content: contentHost.data

	// Customizable directional crescent rim-light, outer halo and optional inner eclipse vignette.
	property color rimColor: Theme.dark ? Qt.rgba(0.68, 0.80, 1.0, 1.0) : Theme.accent
	property color rimCoreColor: Theme.dark ? Qt.rgba(0.95, 0.97, 1.0, 1.0) : Theme.accentText
	property real rimBiasAngle: NaN
	property real rimStrengthScale: 1.0
	property real eclipseShade: 0.0

	readonly property real haloPad: 10
	readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)
	readonly property real halfWidth: Math.max(1, width * 0.5)
	readonly property real halfHeight: Math.max(1, height * 0.5)
	readonly property real cursorNx: hover.hovered ? Math.max(-1, Math.min(1, (hover.point.position.x - halfWidth) / halfWidth)) : 0
	readonly property real cursorNy: hover.hovered ? Math.max(-1, Math.min(1, (hover.point.position.y - halfHeight) / halfHeight)) : 0

	property real targetGlowAngle: isNaN(rimBiasAngle) ? Math.PI * 0.75 : rimBiasAngle
	property real glowAngle: targetGlowAngle
	property real glowStrength: hover.hovered ? 1.0 : 0.0
	property real pressPulse: tap.pressed ? 1.0 : 0.0
	property real eclipseProgress: eclipseShade

	implicitWidth: 34
	implicitHeight: 34

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
	Behavior on eclipseProgress {
		NumberAnimation {
			duration: Motion.standard
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

	function desiredGlowAngle() {
		const hasBias = !isNaN(rimBiasAngle)
		const mag2 = cursorNx * cursorNx + cursorNy * cursorNy
		if (hasBias) {
			if (!hover.hovered || mag2 <= 0.01)
				return rimBiasAngle
			const cur = Math.atan2(cursorNy, cursorNx)
			let diff = cur - rimBiasAngle
			while (diff > Math.PI)
				diff -= 2 * Math.PI
			while (diff < -Math.PI)
				diff += 2 * Math.PI
			return rimBiasAngle + diff * 0.28
		}
		if (!hover.hovered || mag2 <= 0.01)
			return targetGlowAngle
		return Math.atan2(cursorNy, cursorNx)
	}

	function updateGlowTarget() {
		const raw = desiredGlowAngle()
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

	function scheduleEclipsePaint() {
		if (eclipseCanvas.visible)
			eclipseCanvas.requestPaint()
	}

	onRimBiasAngleChanged: updateGlowTarget()
	onGlowAngleChanged: {
		scheduleRimPaint()
		scheduleEclipsePaint()
	}
	onGlowStrengthChanged: scheduleRimPaint()
	onPressPulseChanged: scheduleRimPaint()
	onRimColorChanged: {
		scheduleRimPaint()
		scheduleEclipsePaint()
	}
	onRimCoreColorChanged: {
		scheduleRimPaint()
		scheduleEclipsePaint()
	}
	onRimStrengthScaleChanged: scheduleRimPaint()
	onEclipseProgressChanged: scheduleEclipsePaint()
	onPixelScaleChanged: {
		scheduleRimPaint()
		scheduleEclipsePaint()
	}

	Connections {
		target: Theme
		function onDarkChanged() {
			button.scheduleRimPaint()
			button.scheduleEclipsePaint()
		}
	}

	Rectangle {
		id: face
		anchors.fill: parent
		radius: width / 2
		color: hover.hovered ? Theme.surfaceHover : Theme.surface
		border.color: hover.hovered
			? Qt.tint(Theme.border, Qt.alpha(button.rimColor, Theme.dark ? 0.44 : 0.52))
			: Theme.border
		scale: tap.pressed ? 0.90 : (hover.hovered ? 1.06 : 1.0)

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on scale {
			SpringAnimation {
				spring: 5.2
				damping: 0.30
				epsilon: 0.002
			}
		}

		transform: Translate {
			x: hover.hovered ? button.cursorNx * 1.5 : 0
			y: hover.hovered ? button.cursorNy * 1.5 : 0

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

		// Soft directional ambient aura + crisp outer crescent rim masked naturally by `face`.
		// Strictly hidden at rest so idle buttons perform zero Canvas work.
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
			visible: button.glowStrength > 0.005 || button.pressPulse > 0.005

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
				const alpha = Math.max(0, Math.min(1.5, strength))
				const pressBoost = 1.0 + button.pressPulse * 0.28

				// 1. Soft directional ambient halo radiating outward along `glowAngle`.
				const auraX = cx + ca * r * 0.42
				const auraY = cy + sa * r * 0.42
				const auraR = r + button.haloPad * 0.94 * pressBoost
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
				const midShift = 2.2 + button.pressPulse * 0.9
				const midCx = cx + ca * midShift
				const midCy = cy + sa * midShift
				const midApexX = cx + ca * (r + 1.8)
				const midApexY = cy + sa * (r + 1.8)
				const midGrad = ctx.createRadialGradient(midApexX, midApexY, 0, midApexX, midApexY, r * 1.45)
				midGrad.addColorStop(0.0, button.cssRgba(button.rimColor, 0.72 * alpha * pressBoost))
				midGrad.addColorStop(0.55, button.cssRgba(button.rimColor, 0.28 * alpha))
				midGrad.addColorStop(1.0, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = midGrad
				ctx.beginPath()
				ctx.arc(midCx, midCy, r + 1.0, 0, Math.PI * 2)
				ctx.fill()

				// 3. Razor-sharp specular crescent core peeking cleanly around `face`'s edge.
				const coreShift = 1.55 + button.pressPulse * 0.65
				const coreCx = cx + ca * coreShift
				const coreCy = cy + sa * coreShift
				const coreApexX = cx + ca * (r + 1.2)
				const coreApexY = cy + sa * (r + 1.2)
				const coreGrad = ctx.createRadialGradient(coreApexX, coreApexY, 0, coreApexX, coreApexY, r * 1.18)
				coreGrad.addColorStop(0.0, button.cssRgba(button.rimCoreColor, 0.95 * alpha * pressBoost))
				coreGrad.addColorStop(0.45, button.cssRgba(button.rimColor, 0.52 * alpha))
				coreGrad.addColorStop(0.92, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = coreGrad
				ctx.beginPath()
				ctx.arc(coreCx, coreCy, r + 0.45, 0, Math.PI * 2)
				ctx.fill()
			}
		}

		// Optional inner lunar-eclipse shadow sweep (strictly hidden unless `eclipseShade > 0`).
		Canvas {
			id: eclipseCanvas
			z: 0
			x: 0
			y: 0
			width: Math.max(1, Math.ceil(face.width * button.pixelScale))
			height: Math.max(1, Math.ceil(face.height * button.pixelScale))
			transformOrigin: Item.TopLeft
			scale: 1 / button.pixelScale
			renderTarget: Canvas.Image
			visible: button.eclipseProgress > 0.005

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

				const cx = face.width * 0.5
				const cy = face.height * 0.5
				const r = Math.max(1, Math.min(face.width, face.height) * 0.5 - 0.5)
				const p = Math.max(0, Math.min(1, button.eclipseProgress))
				const a = button.glowAngle
				const ca = Math.cos(a)
				const sa = Math.sin(a)

				ctx.save()
				ctx.beginPath()
				ctx.arc(cx, cy, r, 0, Math.PI * 2)
				ctx.clip()

				// Creeping shadow arc entering from `glowAngle`.
				const sx = cx + ca * r * (1.22 - 0.34 * p)
				const sy = cy + sa * r * (1.22 - 0.34 * p)
				const sr = r * (0.95 + 0.55 * p)
				const shadow = ctx.createRadialGradient(sx, sy, 0, sx, sy, sr)
				shadow.addColorStop(0.0, button.cssRgba(button.rimCoreColor, 0.34 * p))
				shadow.addColorStop(0.55, button.cssRgba(button.rimColor, 0.16 * p))
				shadow.addColorStop(1.0, button.cssRgba(button.rimColor, 0))
				ctx.fillStyle = shadow
				ctx.fillRect(0, 0, face.width, face.height)

				// Subtle darkened inner vignette pulling light inward toward center.
				const vignette = ctx.createRadialGradient(cx, cy, r * 0.52, cx, cy, r)
				vignette.addColorStop(0.0, button.cssRgba(button.rimCoreColor, 0))
				vignette.addColorStop(1.0, button.cssRgba(button.rimCoreColor, 0.14 * p))
				ctx.fillStyle = vignette
				ctx.fillRect(0, 0, face.width, face.height)

				ctx.restore()
			}
		}

		Item {
			id: contentHost
			z: 1
			anchors.fill: parent
			scale: tap.pressed ? 0.94 : (hover.hovered ? 1.07 : 1.0)

			Behavior on scale {
				SpringAnimation {
					spring: 5.0
					damping: 0.28
					epsilon: 0.002
				}
			}

			transform: Translate {
				x: hover.hovered ? button.cursorNx * 1.6 : 0
				y: hover.hovered ? button.cursorNy * 1.6 : 0

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
		}
	}

	HoverHandler {
		id: hover
		enabled: button.enabled
		cursorShape: Qt.PointingHandCursor
		onHoveredChanged: button.updateGlowTarget()
		onPointChanged: button.updateGlowTarget()
	}
	TapHandler {
		id: tap
		enabled: button.enabled
		onTapped: button.clicked()
	}

	Accessible.role: Accessible.Button
	Accessible.onPressAction: button.clicked()
}
