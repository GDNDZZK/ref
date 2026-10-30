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

	Item {
		id: page
		anchors.fill: parent

		// The window color is not part of a grab.
		Rectangle {
			anchors.fill: parent
			color: Theme.background
		}

		Loader {
			anchors.fill: parent
			active: Workspace.current !== null

			sourceComponent: SessionPane {
				session: Workspace.current
			}
		}

		// Connection trouble, or why no session could be opened, until it is gone.
		StatusPill {
			text: Workspace.connectionText !== "" ? Workspace.connectionText : Workspace.current === null ? Workspace.failureText : ""
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
}
