// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

Window {
	id: window

	width: 980
	height: 760
	minimumWidth: 520
	minimumHeight: 420
	visible: true
	title: "ref"
	color: Theme.background

	// Swaps the theme under a snapshot of the old look and fades the snapshot out,
	// so every color crossfades at once without per-item color animations.
	function switchTheme() {
		const size = Qt.size(page.width * Screen.devicePixelRatio, page.height * Screen.devicePixelRatio)
		const grabbing = page.grabToImage(
			result => {
				themeSnapshot.grab = result // result.url stays valid only while result is alive
				themeSnapshot.source = result.url
				themeSnapshot.opacity = 1
				Theme.toggle()
				snapshotFade.restart()
			},
		size)
		if (!grabbing)
			Theme.toggle()
	}

	required property ChatSession session

	Item {
		id: page
		anchors.fill: parent

		// The window color is not part of a grab.
		Rectangle {
			anchors.fill: parent
			color: Theme.background
		}

		ChatView {
			id: chat
			anchors { left: parent.left; right: parent.right; top: parent.top; bottom: composer.top }
			model: window.session.blocks
			turnInProgress: window.session.busy
		}

		Text {
			anchors.centerIn: chat
			text: qsTr("What are we building?")
			color: Theme.textMuted
			font.pixelSize: 22
			opacity: chat.count === 0 ? 1 : 0

			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		// Carries an alert badge after an error arrived out of sight, until the button is
		// gone. The badge stays while the button fades out, so it does not pop away first.
		CircleButton {
			id: jumpToEnd

			readonly property bool shown: !chat.following
			property bool alerting: false

			// Stacked above the send button, on its center line.
			x: composer.x + composer.sendButtonCenterX - width / 2
			anchors { bottom: composer.top; bottomMargin: 12 }
			implicitWidth: 34
			implicitHeight: 34
			opacity: shown ? 1 : 0
			visible: opacity > 0
			transform: Translate {
				y: jumpToEnd.shown ? 0 : 10
				Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
			}
			Accessible.name: qsTr("Jump to latest")

			onClicked: chat.glideToEnd()
			onVisibleChanged: {
				if (visible)
					return
				alerting = false
				alert.opacity = 0 // the next alert enters again
			}

			Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Connections {
				target: chat
				// A later error while the badge is shown springs it again.
				function onErrorOutOfSight() {
					if (jumpToEnd.alerting) {
						alertFlourish.play()
						return
					}
					jumpToEnd.alerting = true
					alertFlourish.enter()
				}
			}

			Chevron {
				anchors.centerIn: parent
				width: 16
				height: 16
				rotation: 90 // points down
				color: Theme.textMuted
				strokeWidth: 2
			}

			Flourish {
				id: alertFlourish
				target: alert
				color: Theme.danger
				ringReach: 2.5
			}

			// On the button's rim, at its upper right. The background-colored border
			// cuts it out of the button.
			AlertBadge {
				id: alert
				readonly property real rimOffset: jumpToEnd.width / 2 * Math.SQRT1_2 // from the button's center at 45°
				x: jumpToEnd.width / 2 + rimOffset - width / 2
				y: jumpToEnd.height / 2 - rimOffset - height / 2
				implicitWidth: 16
				implicitHeight: 16
				border.color: Theme.background
				border.width: 2
				opacity: 0 // the Flourish fades it in
				visible: jumpToEnd.alerting
			}
		}

		Composer {
			id: composer
			anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: Theme.pagePadding }
			width: chat.columnWidth
			busy: window.session.busy
			onSubmitted: text => {
				chat.following = true
				window.session.send(text)
			}
			onStopRequested: window.session.stop()
		}
	}

	Image {
		id: themeSnapshot

		property var grab: null

		anchors.fill: parent
		opacity: 0
		visible: opacity > 0
	}

	NumberAnimation {
		id: snapshotFade
		target: themeSnapshot
		property: "opacity"
		to: 0
		duration: Motion.slow
		easing.type: Motion.ease
		onFinished: {
			themeSnapshot.source = ""
			themeSnapshot.grab = null
		}
	}

	// Above the snapshot, so its own morph stays visible during the crossfade.
	ThemeToggle {
		anchors { top: parent.top; right: parent.right; margins: 14 }
		onClicked: window.switchTheme()
	}

	Component.onCompleted: composer.focusInput()
}
