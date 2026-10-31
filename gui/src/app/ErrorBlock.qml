// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Why a reply failed or was cut short. Errors are rare and must not be missed.
//
// The card drops in neutral. Once it lands, its badge fades and springs in with a
// Flourish, and the error tint spreads out of it with it, as a hard-edged circle at
// a constant speed until it covers the card.
Block {
	id: block

	// 0 = neutral card, 1 = covered by the tint. Rows created with the view start covered.
	property real spread: 1

	// px per ms the circle's edge travels: a full-width card (about 760 px from the badge to the
	// far corner) is covered in about half a second.
	readonly property real spreadSpeed: 1.5

	onAdded: {
		arrival.restart()
		if (!chat.following)
			chat.errorOutOfSight()
	}

	Rectangle {
		id: card

		readonly property int verticalPadding: 12
		readonly property int horizontalPadding: 14

		width: block.columnWidth
		height: message.height + 2 * verticalPadding
		radius: Theme.radius
		color: block.spread < 1 ? Theme.surface : Theme.errorSurface
		border.color: Qt.tint(Theme.border, Qt.alpha(Theme.errorBorder, block.spread))

		// The circle, drawn as a radial gradient with a one-pixel step at its edge, clipped to the
		// card's rounded outline. Inset by the border, which stays on top.
		Canvas {
			id: wash

			readonly property real centerX: badge.x + badge.width / 2
			readonly property real centerY: badge.y + badge.height / 2
			readonly property real startRadius: badge.width / 2
			readonly property real endRadius: Math.hypot(Math.max(centerX, width - centerX), Math.max(centerY, height - centerY))
			readonly property real radius: startRadius + block.spread * (endRadius - startRadius)
			readonly property real reach: endRadius + 1 // gradient radius: the step stays within it

			anchors.fill: parent
			renderTarget: Canvas.Image
			visible: block.spread > 0 && block.spread < 1

			onRadiusChanged: requestPaint()
			onVisibleChanged: requestPaint()
			onPaint: {
				const ctx = getContext("2d")
				ctx.reset()
				if (!visible)
					return
				const inset = card.border.width
				const rounding = card.radius - inset
				const gradient = ctx.createRadialGradient(centerX, centerY, 0, centerX, centerY, reach)
				gradient.addColorStop(Math.min(1, radius / reach), Theme.errorSurface)
				gradient.addColorStop(Math.min(1, (radius + 1) / reach), Qt.alpha(Theme.errorSurface, 0))
				ctx.fillStyle = gradient
				ctx.beginPath()
				ctx.roundedRect(inset, inset, width - 2 * inset, height - 2 * inset, rounding, rounding)
				ctx.fill()
			}
		}

		FontMetrics {
			id: metrics
			font: message.font
		}

		Flourish {
			id: flourish
			target: badge
			color: Theme.danger
			ringReach: 2.2 // stays inside a one-line card
		}

		// Level with the first line of the message.
		AlertBadge {
			id: badge
			x: card.horizontalPadding
			y: message.y + message.topPadding + (metrics.height - height) / 2
		}

		ProseText {
			id: message
			anchors { left: badge.right; leftMargin: 10; right: parent.right; rightMargin: card.horizontalPadding }
			y: card.verticalPadding
			textFormat: TextEdit.PlainText
			text: block.content
		}
	}

	SequentialAnimation {
		id: arrival

		PropertyAction { target: badge; property: "opacity"; value: 0 } // the Flourish fades it in
		PropertyAction { target: block; property: "spread"; value: 0 }
		PauseAnimation { duration: Motion.quick } // lets the drop land first
		ScriptAction { script: flourish.enter() } // the circle leaves as the badge springs open
		NumberAnimation {
			target: block
			property: "spread"
			to: 1
			duration: (wash.endRadius - wash.startRadius) / block.spreadSpeed
			easing.type: Easing.Linear
		}
	}
}
