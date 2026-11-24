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
	property bool allowDirtyWindowClose: false
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
		if (session !== shown)
			shown = session
	}

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
		const p = item.mapFromItem(null, scenePos.x, scenePos.y)
		return p.x >= 0 && p.y >= 0 && p.x <= item.width && p.y <= item.height
	}

	function overTopButtons(scenePos) {
		return pointInItem(expand, scenePos)
			|| pointInItem(newSessionButton, scenePos)
			|| pointInItem(splitFilterCapsule, scenePos)
			|| pointInItem(leaveZoomButton, scenePos)
			|| pointInItem(splitButton, scenePos)
			|| pointInItem(floatButton, scenePos)
			|| pointInItem(themeToggle, scenePos)
			|| pointInItem(windowControls, scenePos)
	}

	title: "ref"
	flags: isMac
		? (Qt.Window | Qt.ExpandedClientAreaHint | Qt.NoTitleBarBackgroundHint)
		: (Qt.Window | Qt.FramelessWindowHint | Qt.WindowMinMaxButtonsHint | Qt.WindowSystemMenuHint)
	minimumWidth: Theme.windowMinWidth
	minimumHeight: Theme.windowMinHeight
	visible: !Workspace.floatingMode
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
	onClosing: close => {
		if (Workspace.smoke || window.allowDirtyWindowClose)
			return
		if (!dialogs.requestWindowClose())
			close.accepted = false
	}

	Binding {
		target: Workspace
		property: "windowActive"
		value: window.active || (floatingBallLoader.item ? floatingBallLoader.item.panelActive : false)
	}
	Binding {
		target: Theme
		property: "dark"
		value: window.wantsDark
	}

	Binding {
		target: UiScale
		property: "factor"
		value: Workspace.settings.uiScale / Workspace.startupUiScale
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
		function onFloatingModeChanged() {
			if (!Workspace.floatingMode) {
				window.show()
				window.raise()
				window.requestActivate()
				Qt.callLater(() => window.focusComposer())
			}
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
			readonly property bool isZoomCommand: model.commandId === CommandId.uiZoomIn
				|| model.commandId === CommandId.uiZoomOut
				|| model.commandId === CommandId.uiZoomReset

			sequence: model.shortcut
			enabled: model.shortcut !== "" && model.enabled && (
				Workspace.dialog === ""
				|| (Workspace.dialog === DialogId.palette && model.commandId === CommandId.palette)
				|| (Workspace.dialog !== DialogId.palette && isZoomCommand)
			)
			autoRepeat: false
			onActivated: {
				if (Workspace.dialog === DialogId.palette && model.commandId === CommandId.palette)
					Workspace.closeDialog()
				else
					Commands.run(model.commandId)
			}
		}
	}
	Shortcut {
		sequences: ["Ctrl++", "Ctrl+Shift+="]
		enabled: Workspace.dialog !== DialogId.palette
		autoRepeat: false
		onActivated: Commands.run(CommandId.uiZoomIn)
	}
	Shortcut {
		sequence: "Ctrl+T"
		enabled: Workspace.dialog === "" && Workspace.frontSession !== null && Workspace.frontSession.composer.variantAvailable
		autoRepeat: false
		onActivated: Workspace.setVariant(Workspace.frontSession.sessionId, "")
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
		id: scaledRoot
		objectName: "scaledRoot"
		transformOrigin: Item.TopLeft
		scale: UiScale.factor
		width: window.width / scale
		height: window.height / scale
	Item {
		id: page
		anchors.fill: parent
		enabled: !dialogs.open
		readonly property int minAreaWidth: 320

		// The window color is not part of a grab.
		Rectangle {
			anchors.fill: parent
			color: Theme.background
		}

		Sidebar {
			id: sidebar
			height: parent.height
			preferredWidth: Workspace.settings.sidebarWidth
			maxAllowed: Math.max(Theme.sidebarMinWidth, Math.min(Theme.sidebarMaxWidth, (page.width > 0 ? page.width : Workspace.settings.windowWidth) - page.minAreaWidth))
			width: Workspace.settings.sidebarVisible ? contentWidth : 0
			visible: width > 0
			headerLeftInset: window.isMac ? window.macTrafficLightInset : 0
			dragEnabled: !dialogs.visible
			onCollapseRequested: Commands.run(CommandId.sidebar)
			onMoveRequested: window.startSystemMove()
			onToggleMaximizeRequested: window.toggleMaximized()
			Behavior on width {
				enabled: window.restored && !(sidebar.resizing && Workspace.settings.sidebarVisible)
				NumberAnimation { id: sidebarWidthAnim; duration: Motion.standard; easing.type: Motion.ease }
			}
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
							layoutWidth: sidebarWidthAnim.running ? Math.max(0, page.width - sidebar.contentWidth) : width
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
						sidebarAnimating: sidebarWidthAnim.running
						minAreaWidth: Math.max(0, page.width - sidebar.contentWidth)
						visible: (Workspace.splitView || occupied) && opacity > 0
						enabled: Workspace.splitView
					}

					// Connection trouble, or why no session could be opened, until it is gone.
					StatusPill {
						text: window.noticeText()
						actionText: Workspace.connectionText !== "" ? qsTr("Switch Backend…") : ""
						onActionClicked: Commands.run(CommandId.backend)
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

				Rectangle {
					id: splitFilterCapsule
					objectName: "splitFilterCapsule"

					readonly property real leftClear: sidebar.width === 0
						? ((window.isMac ? window.macTrafficLightInset : Theme.windowButtonMargin) + 2 * Theme.buttonHeight + Theme.windowButtonGap + 8)
						: 12
					readonly property real rightClear: (window.showWindowControls ? windowControls.width + Theme.windowButtonGap : 0)
						+ themeToggle.width + floatButton.width + splitButton.width
						+ 3 * Theme.windowButtonGap + Theme.windowButtonMargin + 8
					readonly property real maxWidth: Math.max(0, area.width - leftClear - rightClear)
					readonly property bool shown: Workspace.splitView && !window.zoomed && maxWidth >= 96

					anchors.horizontalCenter: parent.horizontalCenter
					y: window.topButtonY
					z: 5
					width: Math.min(splitFilterRow.implicitWidth + 6, maxWidth)
					height: Theme.buttonHeight
					radius: height / 2
					color: Theme.surface
					border.color: Theme.border
					opacity: shown ? 1 : 0
					visible: opacity > 0
					enabled: !dialogs.open && shown
					clip: true

					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

					Row {
						id: splitFilterRow
						anchors.centerIn: parent
						spacing: 2

						Rectangle {
							id: filterAllPill
							objectName: "splitFilterAll"
							readonly property bool active: Workspace.splitFilter === "all"
							width: filterAllText.implicitWidth + 20
							height: 28
							radius: 14
							color: active ? Theme.userBubble : (filterAllHover.hovered ? Theme.surfaceHover : "transparent")
							border.color: active ? Theme.userBubbleBorder : "transparent"

							Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							Text {
								id: filterAllText
								textFormat: Text.PlainText
								anchors.centerIn: parent
								text: qsTr("All")
								color: filterAllPill.active || filterAllHover.hovered ? Theme.text : Theme.textMuted
								font.pixelSize: Theme.smallFontSize
							}
							HoverHandler { id: filterAllHover; cursorShape: Qt.PointingHandCursor }
							TapHandler { onTapped: Workspace.setSplitFilter("all") }
						}

						Rectangle {
							id: filterUserPill
							objectName: "splitFilterUser"
							readonly property bool active: Workspace.splitFilter === "user"
							width: filterUserText.implicitWidth + 20
							height: 28
							radius: 14
							color: active ? Theme.userBubble : (filterUserHover.hovered ? Theme.surfaceHover : "transparent")
							border.color: active ? Theme.userBubbleBorder : "transparent"

							Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							Text {
								id: filterUserText
								textFormat: Text.PlainText
								anchors.centerIn: parent
								text: qsTr("User-created")
								color: filterUserPill.active || filterUserHover.hovered ? Theme.text : Theme.textMuted
								font.pixelSize: Theme.smallFontSize
							}
							HoverHandler { id: filterUserHover; cursorShape: Qt.PointingHandCursor }
							TapHandler { onTapped: Workspace.setSplitFilter("user") }
						}

						Rectangle {
							id: filterCreatorPill
							objectName: "splitFilterCreator"
							readonly property bool active: Workspace.splitFilter !== "all" && Workspace.splitFilter !== "user"
							visible: active
							width: Math.min(150, filterCreatorText.implicitWidth + 20)
							height: 28
							radius: 14
							color: Theme.userBubble
							border.color: Theme.userBubbleBorder

							Text {
								id: filterCreatorText
								textFormat: Text.PlainText
								anchors { left: parent.left; right: parent.right; leftMargin: 10; rightMargin: 10; verticalCenter: parent.verticalCenter }
								text: qsTr("By %1 ×").arg(Workspace.splitFilterLabel)
								color: Theme.text
								font.pixelSize: Theme.smallFontSize
								elide: Text.ElideRight
							}
							HoverHandler { id: filterCreatorHover; cursorShape: Qt.PointingHandCursor }
							TapHandler { onTapped: Workspace.setSplitFilter("all") }
							PlainToolTip {
								visible: filterCreatorHover.hovered
								text: qsTr("Clear creator filter")
							}
						}

						Item {
							id: filterMoreButton
							objectName: "splitFilterMenuButton"
							visible: Workspace.splitCreators.length > 0
							width: 28
							height: 28

							Rectangle {
								anchors.fill: parent
								radius: 14
								color: filterMoreHover.hovered ? Theme.surfaceHover : "transparent"
								Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
							}
							Icon {
								anchors.centerIn: parent
								name: "filter"
								size: 16
								color: filterMoreHover.hovered ? Theme.text : Theme.textMuted
							}
							HoverHandler { id: filterMoreHover; cursorShape: Qt.PointingHandCursor }
							TapHandler {
								onTapped: {
									const creators = Workspace.splitCreators
									const items = [
										{text: qsTr("All"), action: "all"},
										{text: qsTr("User-created"), action: "user"}
									]
									for (let i = 0; i < creators.length; ++i)
										items.push({text: qsTr("By %1").arg(creators[i].name), action: creators[i].id})
									splitFilterMenu.entries = items
									splitFilterMenu.openAt(Qt.point(0, filterMoreButton.height + 6))
								}
							}
							PlainToolTip {
								visible: filterMoreHover.hovered
								text: qsTr("Filter by creator")
							}
							ContextMenu {
								id: splitFilterMenu
								onTriggered: index => {
									const entry = splitFilterMenu.entries[index]
									if (entry)
										Workspace.setSplitFilter(entry.action)
								}
							}
						}
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
		enabled: !dialogs.open
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
		enabled: !dialogs.open
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
		enabled: !dialogs.open
		onClicked: Workspace.requestTheme(Theme.dark ? ThemeName.light : ThemeName.dark)
	}

	IconButton {
		id: floatButton
		objectName: "floatingBallButton"
		anchors {
			top: parent.top
			right: themeToggle.left
			topMargin: window.topButtonY
			rightMargin: Theme.windowButtonGap
		}
		iconName: "float"
		iconColor: Workspace.floatingMode ? Theme.accentText : Theme.textMuted
		enabled: !dialogs.open
		tooltipText: qsTr("Floating ball (Ctrl+Shift+F)")
		Accessible.name: qsTr("Floating ball")
		onClicked: Commands.run(CommandId.floating)
	}

	IconButton {
		id: splitButton
		anchors {
			top: parent.top
			right: floatButton.left
			topMargin: window.topButtonY
			rightMargin: Theme.windowButtonGap
		}
		iconName: "split"
		iconColor: Workspace.splitView ? Theme.accentText : Theme.textMuted
		enabled: !dialogs.open
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
		enabled: !dialogs.open
		Accessible.name: qsTr("Leave zoom")
		onClicked: Workspace.unzoom()

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}
	DialogHost {
		id: dialogs
		anchors.fill: parent
		onQuitWindowConfirmed: {
			window.allowDirtyWindowClose = true
			window.close()
		}
	}
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

	Loader {
		id: floatingBallLoader
		active: Workspace.floatingMode
		sourceComponent: FloatingBallWindow {}
	}
}
