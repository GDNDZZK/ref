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
	readonly property bool isMac: Qt.platform.os === "osx"
	readonly property bool showWindowControls: !isMac
	readonly property int macTrafficLightInset: 76
	readonly property bool windowBorderShown: !isMac && visibility !== Window.Maximized && visibility !== Window.FullScreen
	readonly property int topButtonY: Math.round((Theme.sidebarHeaderHeight - Theme.buttonHeight) / 2)
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
		if (session === shown)
			return
		if (session === null) {
			if (Workspace.splitView || !content.visible)
				shown = null
			else
				sessionFade.run(content, () => shown = null)
			return
		}
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

	function toggleMaximized() {
		if (visibility === Window.Maximized || visibility === Window.FullScreen)
			showNormal()
		else
			showMaximized()
	}

	function pointInItem(item, scenePos) {
		if (!item.visible)
			return false
		const p = page.mapToItem(item, scenePos.x, scenePos.y)
		return p.x >= 0 && p.y >= 0 && p.x <= item.width && p.y <= item.height
	}

	function overTopButtons(scenePos) {
		return pointInItem(expand, scenePos)
			|| pointInItem(newSessionButton, scenePos)
			|| pointInItem(leaveZoomButton, scenePos)
			|| pointInItem(splitButton, scenePos)
			|| pointInItem(themeToggle, scenePos)
			|| pointInItem(windowControls, scenePos)
	}

	title: "ref"
	flags: isMac
		? (Qt.Window | Qt.ExpandedClientAreaHint | Qt.NoTitleBarBackgroundHint)
		: (Qt.Window | Qt.FramelessWindowHint | Qt.WindowMinMaxButtonsHint | Qt.WindowSystemMenuHint)
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
		enabled: !dialogs.open

		// The window color is not part of a grab.
		Rectangle {
			anchors.fill: parent
			color: Theme.background
		}

		Sidebar {
			id: sidebar
			height: parent.height
			width: Workspace.settings.sidebarVisible ? Theme.sidebarWidth : 0
			headerLeftInset: window.isMac ? window.macTrafficLightInset : 0
			dragEnabled: !dialogs.visible
			onCollapseRequested: Commands.run(CommandId.sidebar)
			onMoveRequested: window.startSystemMove()
			onToggleMaximizeRequested: window.toggleMaximized()

			Behavior on width { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		}

		Item {
			id: area
			x: sidebar.width
			width: parent.width - x
			height: parent.height

			Item {
				id: areaBody
				y: Theme.sidebarHeaderHeight
				width: parent.width
				height: parent.height - y
				clip: true

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
						opacity: Workspace.splitView ? 0 : 1
						visible: opacity > 0
						enabled: !Workspace.splitView

						Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

						sourceComponent: SessionPane {
							session: window.shown
						}
					}

					Column {
						id: emptyState
						readonly property bool active: window.shown === null
							&& Workspace.connectionState === "connected"
							&& Workspace.failureText === ""

						anchors.centerIn: parent
						width: Math.min(parent.width - 2 * Theme.pagePadding, Theme.contentWidth)
						spacing: 12
						opacity: Workspace.splitView ? 0 : 1
						visible: active && opacity > 0
						enabled: active && !Workspace.splitView
						Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

						Text {
							textFormat: Text.PlainText
							anchors.horizontalCenter: parent.horizontalCenter
							text: qsTr("No session open")
							color: Theme.textMuted
							font.pixelSize: Theme.fontSize
						}

						Row {
							anchors.horizontalCenter: parent.horizontalCenter
							spacing: 8

							TextButton {
								text: qsTr("New session in folder…")
								onClicked: Commands.run(CommandId.newSessionIn)
							}

							TextButton {
								text: qsTr("New session")
								onClicked: Commands.run(CommandId.newSession)
							}
						}

						Text {
							id: emptyCwdLabel
							textFormat: Text.PlainText
							anchors.horizontalCenter: parent.horizontalCenter
							width: Math.min(implicitWidth, parent.width)
							text: Format.dirName(Workspace.cwd) !== Workspace.cwd && Format.dirName(Workspace.cwd) !== ""
								? Format.dirName(Workspace.cwd) + " · " + Workspace.cwd
								: Workspace.cwd
							color: Theme.textMuted
							font.pixelSize: Theme.smallFontSize
							font.family: Theme.monoFamily
							elide: Text.ElideMiddle
							visible: Workspace.cwd !== ""

							HoverHandler { id: emptyCwdHover }
							PlainToolTip {
								visible: emptyCwdHover.hovered && emptyCwdLabel.truncated
								text: Workspace.cwd
							}
						}
					}

					TileGrid {
						id: tiles
						anchors.fill: parent
						topInset: 0
						visible: (Workspace.splitView || occupied) && opacity > 0
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
			}

			Rectangle {
				id: titleBar
				width: parent.width
				height: Theme.sidebarHeaderHeight
				color: Theme.background

				DragHandler {
					enabled: !dialogs.visible
					target: null
					dragThreshold: 4
					onActiveChanged: {
						if (active && !window.overTopButtons(centroid.scenePosition))
							window.startSystemMove()
					}
				}
				TapHandler {
					enabled: !dialogs.visible
					onDoubleTapped: eventPoint => {
						if (!window.overTopButtons(eventPoint.scenePosition))
							window.toggleMaximized()
					}
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
		anchors {
			top: parent.top
			left: parent.left
			topMargin: window.topButtonY
			leftMargin: window.isMac ? window.macTrafficLightInset : Theme.windowButtonMargin
		}
		iconName: "sidebar"
		opacity: sidebar.width === 0 ? 1 : 0
		visible: opacity > 0
		Accessible.name: qsTr("Show sidebar")
		onClicked: Commands.run(CommandId.sidebar)

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	IconButton {
		id: newSessionButton
		anchors {
			top: parent.top
			left: expand.right
			leftMargin: Theme.windowButtonGap
			topMargin: window.topButtonY
		}
		iconName: "plus"
		opacity: expand.opacity
		visible: opacity > 0
		Accessible.name: qsTr("New session")
		onClicked: Commands.run(CommandId.newSession)
	}

	WindowControls {
		id: windowControls
		anchors {
			top: parent.top
			right: parent.right
			topMargin: window.topButtonY
			rightMargin: Theme.windowButtonMargin
		}
		visible: window.showWindowControls
		maximized: window.visibility === Window.Maximized || window.visibility === Window.FullScreen
		onMinimizeRequested: window.showMinimized()
		onMaximizeRequested: window.toggleMaximized()
		onCloseRequested: window.close()
	}

	ThemeToggle {
		id: themeToggle
		anchors {
			top: parent.top
			right: windowControls.visible ? windowControls.left : parent.right
			topMargin: window.topButtonY
			rightMargin: windowControls.visible ? Theme.windowButtonGap : Theme.windowButtonMargin
		}
		onClicked: Workspace.requestTheme(Theme.dark ? ThemeName.light : ThemeName.dark)
	}

	IconButton {
		id: splitButton
		anchors {
			top: parent.top
			right: themeToggle.left
			topMargin: window.topButtonY
			rightMargin: Theme.windowButtonGap
		}
		iconName: "split"
		iconColor: Workspace.splitView ? Theme.accentText : Theme.textMuted
		Accessible.name: qsTr("Split view")
		onClicked: Commands.run(CommandId.watch)
	}

	IconButton {
		id: leaveZoomButton
		anchors {
			top: parent.top
			right: splitButton.left
			topMargin: window.topButtonY
			rightMargin: Theme.windowButtonGap
		}
		iconName: "restore"
		opacity: window.zoomed ? 1 : 0
		visible: opacity > 0
		Accessible.name: qsTr("Leave zoom")
		onClicked: Workspace.unzoom()

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	DialogHost {
		id: dialogs
		anchors.fill: parent
	}

	WindowResizeHandles {
		anchors.fill: parent
		active: !window.isMac && window.visibility === Window.Windowed
		onResizeRequested: edges => window.startSystemResize(edges)
	}

	Rectangle {
		anchors.fill: parent
		color: "transparent"
		border.width: 1
		border.color: Theme.border
		visible: window.windowBorderShown
	}
}
