pragma ComponentBehavior: Bound
// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import QtQml
import QtQuick.Dialogs
import RefGui

Window {
	id: window

	// The session the content area shows. It follows Workspace.current under a crossfade.
	property var shown: null
	property bool restored: false
	readonly property bool zoomed: Workspace.zoomedId !== ""
	// The theme setting decides; "system" follows the system's color scheme.
	readonly property bool wantsDark: Workspace.settings.theme === ThemeName.dark
		|| (Workspace.settings.theme === ThemeName.system && Application.styleHints.colorScheme === Qt.Dark)

	// Connection trouble, or why no session could be opened; "" when there is nothing to say.
	function noticeText() {
		if (Workspace.connectionText !== "")
			return Workspace.connectionText
		return Workspace.current === null ? Workspace.failureText : ""
	}

	function showSession(session) {
		if (session === null)
			return
		if (shown === null || Workspace.splitView || !content.visible) {
			shown = session
			return
		}
		sessionFade.run(content, () => shown = session)
	}

	// A session that is shown is deleted when it is closed, and the crossfade to the next one takes a
	// frame or more to get going. What is shown moves on at once, so the pane never meets a deleted session.
	Connections {
		target: Workspace

		function onSessionDropped(session) {
			if (window.shown === session)
				window.shown = Workspace.current
		}
	}

	// Puts the keyboard back in the input box of the session in front.
	function focusComposer() {
		if (Workspace.splitView)
			tiles.focusFocused()
		else if (single.item)
			(single.item as SessionPane).focusInput()
	}

	title: "ref"
	minimumWidth: Theme.windowMinWidth
	minimumHeight: Theme.windowMinHeight
	visible: true
	color: Theme.background

	Component.onCompleted: {
		width = Workspace.settings.windowWidth
		height = Workspace.settings.windowHeight
		restored = true
		showSession(Workspace.current)
		if (Workspace.settings.loadProblem !== "")
			Workspace.showToast(ToastLevel.warn, Workspace.settings.loadProblem)
	}
	onWidthChanged: if (restored && visibility === Window.Windowed) Workspace.settings.windowWidth = width
	onHeightChanged: if (restored && visibility === Window.Windowed) Workspace.settings.windowHeight = height

	Binding {
		target: Workspace
		property: "windowActive"
		value: window.active
	}

	Binding {
		target: Theme
		property: "dark"
		value: window.wantsDark
	}

	Connections {
		target: Workspace

		function onCurrentChanged() {
			window.showSession(Workspace.current)
		}
		function onThemeChangeRequested(theme) {
			themeFade.run(page, () => Workspace.settings.theme = theme)
		}
		function onDialogChanged() {
			if (Workspace.dialog === "")
				window.focusComposer()
		}
		function onNewSessionFolderRequested() {
			folders.open()
		}
	}

	FolderDialog {
		id: folders
		title: qsTr("New session in…")
		onAccepted: Workspace.newSession(Format.localPath(selectedFolder))
	}

	// Every command with a key has a shortcut here, so the keys cannot drift from the registry.
	Instantiator {
		model: Commands

		delegate: Shortcut {
			required property var model

			sequence: model.shortcut
			enabled: model.shortcut !== "" && model.enabled
			autoRepeat: false
			onActivated: Commands.run(model.commandId)
		}
	}

	Instantiator {
		model: [
			{ key: "Alt+Left", dx: -1, dy: 0 },
			{ key: "Alt+Right", dx: 1, dy: 0 },
			{ key: "Alt+Up", dx: 0, dy: -1 },
			{ key: "Alt+Down", dx: 0, dy: 1 }
		]

		delegate: Shortcut {
			required property var modelData
			sequence: modelData.key
			enabled: Workspace.splitView && Workspace.dialog === ""
			onActivated: tiles.moveFocus(modelData.dx, modelData.dy)
		}
	}

	Item {
		id: page
		anchors.fill: parent

		// The window color is not part of a grab.
		Rectangle {
			anchors.fill: parent
			color: Theme.background
		}

		Sidebar {
			id: sidebar
			height: parent.height
			width: Workspace.settings.sidebarVisible ? Theme.sidebarWidth : 0
			onCollapseRequested: Commands.run(CommandId.sidebar)

			Behavior on width { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		Item {
			id: area
			x: sidebar.width
			width: parent.width - x
			height: parent.height

			BackendBanner {
				id: banner
			}

			Item {
				id: content
				y: banner.shownHeight
				width: parent.width
				height: parent.height - y

				Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

				Loader {
					id: single
					anchors.fill: parent
					active: window.shown !== null
					// The tile of the current session takes its place at once, and gives it back at once.
					visible: Workspace.tileStage.placed === 0
					enabled: !Workspace.splitView

					sourceComponent: SessionPane {
						session: window.shown
					}
				}

				TileGrid {
					id: tiles
					anchors.fill: parent
					topInset: window.zoomed ? 0 : Math.max(0, themeToggle.y + themeToggle.height - content.y)
					visible: Workspace.splitView || occupied
					enabled: Workspace.splitView
				}

				// Connection trouble, or why no session could be opened, until it is gone.
				StatusPill {
					text: window.noticeText()
				}

				Crossfade {
					id: sessionFade
				}
			}

			ToastHost {
				anchors.fill: parent
			}
		}
	}

	Crossfade {
		id: themeFade
	}

	// Above the snapshot, so their own morph stays visible during the crossfade.
	IconButton {
		id: expand
		anchors { top: parent.top; left: parent.left; margins: Theme.windowButtonMargin }
		iconName: "sidebar"
		opacity: sidebar.width === 0 ? 1 : 0
		visible: opacity > 0
		Accessible.name: qsTr("Show sidebar")
		onClicked: Commands.run(CommandId.sidebar)

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	IconButton {
		anchors { top: parent.top; left: expand.right; leftMargin: Theme.windowButtonGap; topMargin: Theme.windowButtonMargin }
		iconName: "plus"
		opacity: expand.opacity
		visible: opacity > 0
		Accessible.name: qsTr("New session")
		onClicked: Commands.run(CommandId.newSession)
	}

	ThemeToggle {
		id: themeToggle
		anchors { top: parent.top; right: parent.right; margins: Theme.windowButtonMargin }
		onClicked: Workspace.requestTheme(Theme.dark ? ThemeName.light : ThemeName.dark)
	}

	IconButton {
		id: splitButton
		anchors { top: parent.top; right: themeToggle.left; topMargin: Theme.windowButtonMargin; rightMargin: Theme.windowButtonGap }
		iconName: "split"
		iconColor: Workspace.splitView ? Theme.accentText : Theme.textMuted
		Accessible.name: qsTr("Split view")
		onClicked: Commands.run(CommandId.watch)
	}

	IconButton {
		anchors { top: parent.top; right: splitButton.left; topMargin: Theme.windowButtonMargin; rightMargin: Theme.windowButtonGap }
		iconName: "restore"
		opacity: window.zoomed ? 1 : 0
		visible: opacity > 0
		Accessible.name: qsTr("Leave zoom")
		onClicked: Workspace.unzoom()

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	DialogHost {
		anchors.fill: parent
	}
}
