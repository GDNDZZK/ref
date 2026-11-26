import QtQuick
import RefGui

// Text with an angled multi-stop highlight sweep across the glyphs while work continues.
Item {
	id: shimmer

	property string label
	property color color: Theme.textMuted
	property color highlight: Theme.text
	property bool running: true
	property alias font: baseText.font

	readonly property int shimmerPeriod: 1600 // ms for one sweep
	readonly property real sweepAngleDeg: 18
	readonly property real sweepAngleRad: sweepAngleDeg * Math.PI / 180
	readonly property int minSweepSpan: 72
	readonly property real spanWidthRatio: 1.4
	readonly property real midStopRatio: 0.28
	readonly property real midColorWeight: 0.3
	readonly property bool active: running && visible && width > 0 && height > 0 && label !== ""
	readonly property real pixelScale: Math.max(1, (Screen.devicePixelRatio || 1) * UiScale.factor)
	property real phase: 0

	implicitWidth: baseText.implicitWidth
	implicitHeight: baseText.implicitHeight
	baselineOffset: baseText.baselineOffset

	function cssColor(c) {
		return Qt.rgba(c.r, c.g, c.b, c.a).toString()
	}

	function mixColor(a, b, t) {
		return Qt.rgba(
			a.r + (b.r - a.r) * t,
			a.g + (b.g - a.g) * t,
			a.b + (b.b - a.b) * t,
			a.a + (b.a - a.a) * t
		).toString()
	}

	function cssFont() {
		const f = baseText.font
		const weight = f.bold ? "bold " : (f.weight >= Font.Medium ? "500 " : "")
		const style = f.italic ? "italic " : ""
		const px = Math.max(1, Math.round(f.pixelSize > 0 ? f.pixelSize : Theme.fontSize))
		const raw = (f.family || "").trim()
		const lower = raw.toLowerCase()
		const generic = lower === "" || lower === "sans serif" || lower === "sans-serif"
			? "sans-serif"
			: (lower === "monospace" ? "monospace" : null)
		const family = generic !== null ? generic : ("\"" + raw.replace(/"/g, "") + "\", sans-serif")
		return style + weight + px + "px " + family
	}

	function requestCanvasPaint() {
		if (active)
			canvas.requestPaint()
	}

	onLabelChanged: requestCanvasPaint()
	onColorChanged: requestCanvasPaint()
	onHighlightChanged: requestCanvasPaint()
	onActiveChanged: requestCanvasPaint()
	onPixelScaleChanged: requestCanvasPaint()
	onPhaseChanged: requestCanvasPaint()

	Text {
		id: baseText
		anchors.fill: parent
		textFormat: Text.PlainText
		text: shimmer.label
		color: shimmer.color
		visible: !shimmer.active
		onFontChanged: shimmer.requestCanvasPaint()
	}

	FontMetrics {
		id: metrics
		font: baseText.font
	}

	FrameAnimation {
		running: shimmer.active
		onTriggered: {
			shimmer.phase = (shimmer.phase + frameTime * Format.msPerSecond / shimmer.shimmerPeriod) % 1
		}
	}

	Canvas {
		id: canvas
		width: Math.max(1, Math.ceil(shimmer.width * shimmer.pixelScale))
		height: Math.max(1, Math.ceil(shimmer.height * shimmer.pixelScale))
		transformOrigin: Item.TopLeft
		scale: 1 / shimmer.pixelScale
		visible: shimmer.active

		onWidthChanged: shimmer.requestCanvasPaint()
		onHeightChanged: shimmer.requestCanvasPaint()

		onPaint: {
			const ctx = getContext("2d")
			ctx.setTransform(1, 0, 0, 1, 0, 0)
			ctx.clearRect(0, 0, width, height)
			if (shimmer.width <= 0 || shimmer.height <= 0 || shimmer.label === "")
				return
			ctx.setTransform(shimmer.pixelScale, 0, 0, shimmer.pixelScale, 0, 0)

			const w = shimmer.width
			const h = shimmer.height
			const span = Math.max(w * shimmer.spanWidthRatio, shimmer.minSweepSpan)
			const travel = w + span
			const cx = -span * 0.5 + shimmer.phase * travel
			const cy = h * 0.5
			const dx = Math.cos(shimmer.sweepAngleRad) * (span * 0.5)
			const dy = Math.sin(shimmer.sweepAngleRad) * (span * 0.5)

			const grad = ctx.createLinearGradient(cx - dx, cy - dy, cx + dx, cy + dy)
			const baseCss = shimmer.cssColor(shimmer.color)
			const midCss = shimmer.mixColor(shimmer.color, shimmer.highlight, shimmer.midColorWeight)
			const peakCss = shimmer.cssColor(shimmer.highlight)
			grad.addColorStop(0.0, baseCss)
			grad.addColorStop(shimmer.midStopRatio, midCss)
			grad.addColorStop(0.5, peakCss)
			grad.addColorStop(1 - shimmer.midStopRatio, midCss)
			grad.addColorStop(1.0, baseCss)

			ctx.font = shimmer.cssFont()
			ctx.textBaseline = "alphabetic"
			ctx.fillStyle = grad
			const y = metrics.ascent > 0 ? metrics.ascent : h * 0.8
			ctx.fillText(shimmer.label, 0, y)
		}
	}
}
