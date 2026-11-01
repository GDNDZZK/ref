import QtQuick
import RefGui

// One notice of the ToastHost: a card with a glyph and a message that slides in. It tells the host when it is over.
Item {
	id: toast

	required property int toastId
	required property string level
	required property string text
	required property int repeats
	required property bool leaving

	// The lifetime is over or the toast was clicked.
	signal dismissRequested()
	// The toast has faded out; the host drops it.
	signal finished()

	readonly property var looks: ({
		[ToastLevel.error]: {fill: Theme.errorSurface, edge: Theme.errorBorder, tint: Theme.danger, lifetime: Motion.toastErrorMs, icon: "error"},
		[ToastLevel.warn]: {fill: Theme.warningSurface, edge: Theme.warningBorder, tint: Theme.warning, lifetime: Motion.toastWarnMs, icon: "warning"},
		[ToastLevel.info]: {fill: Theme.surface, edge: Theme.border, tint: Theme.accent, lifetime: Motion.toastInfoMs, icon: "info"}
	})
	readonly property var look: looks[level] ?? looks[ToastLevel.info]
	readonly property color fill: look.fill
	readonly property color edge: look.edge
	readonly property color tint: look.tint
	readonly property int lifetime: look.lifetime
	readonly property int cardPaddingX: 12
	readonly property int cardPaddingY: 10
	readonly property int iconGap: 10 // between the glyph and the message

	width: parent.width
	height: leaving ? 0 : card.height + 8
	opacity: leaving ? 0 : 1
	clip: true

	Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on opacity {
		NumberAnimation {
			duration: Motion.standard
			easing.type: Motion.ease
			onRunningChanged: if (!running && toast.leaving) toast.finished()
		}
	}

	onRepeatsChanged: {
		life.restart()
		icon.play()
	}

	Timer {
		id: life
		interval: toast.lifetime
		running: !hover.hovered && !toast.leaving
		onTriggered: toast.dismissRequested()
	}

	Rectangle {
		id: card
		width: parent.width
		height: Math.max(icon.height, message.height) + 2 * toast.cardPaddingY
		radius: Theme.radius
		color: toast.fill
		border.color: toast.edge
		x: entered ? 0 : Motion.toastSlide

		property bool entered: false

		Behavior on x { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Component.onCompleted: {
			entered = true
			icon.enter()
		}

		Item {
			id: icon
			x: toast.cardPaddingX
			y: toast.cardPaddingY
			width: 16
			height: 16

			function enter() {
				flourish.enter()
			}
			function play() {
				flourish.play()
			}

			Flourish {
				id: flourish
				target: glyph
				color: toast.tint
			}
			Icon {
				id: glyph
				name: toast.look.icon
				size: 16
				color: toast.tint
			}
		}

		Text {
			textFormat: Text.PlainText
			id: message
			x: toast.cardPaddingX + icon.width + toast.iconGap
			y: toast.cardPaddingY
			width: parent.width - x - toast.cardPaddingX
			text: toast.text
			color: Theme.text
			font.pixelSize: Theme.captionFontSize
			wrapMode: Text.Wrap
			maximumLineCount: 4
			elide: Text.ElideRight
		}
	}

	HoverHandler { id: hover }
	TapHandler { onTapped: toast.dismissRequested() }
}
