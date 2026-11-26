// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import QtQuick.Controls.Basic as Basic
import RefGui

// Message input of one session. Enter sends, Shift+Enter inserts a line break. While a reply
// is running the send button turns into stop, and what is sent meanwhile waits in the queue
// strip above the box. Under the text are the agent, model and variant of the session.
// The model (session.composer) holds the state; this draws it and turns keys into its actions.
Item {
	id: composer

	required property ChatSession session
	readonly property ComposerModel model: session.composer
	// Whether the input box is there (a split-view tile shows it only while focused). Hidden, it
	// keeps everything it holds; the queue strip above it stays.
	property bool boxShown: true
	property bool ready: false

	readonly property int padding: 8
	readonly property int maxInputHeight: 220
	readonly property int tagRowHeight: 32
	readonly property int textGap: 4
	readonly property int queueGap: 8
	readonly property int inputLeftInset: 4
	readonly property int thumbRowGap: 8 // between the attachment strip and the text
	readonly property real sendButtonCenterX: sendButton.x + sendButton.width / 2 // to line up controls above it

	// A prompt went to the backend.
	signal submitted()
	// A key press, before the input box handles it; accepting the event keeps it from the box.
	signal keyOffered(var event)

	function focusInput() {
		input.forceActiveFocus()
	}

	// Sends the text, or runs it when it is a slash command.
	function submit() {
		const line = model.text.trim()
		const isCommand = !model.shellMode && line.startsWith("/")
		if (isCommand && Commands.runSlash(line, session.sessionId)) {
			model.finishCommand()
			return
		}
		if (model.submit())
			submitted()
	}

	// Whether pressing Enter while `popup.open` should accept the popup item instead of submitting the typed draft.
	function shouldAcceptOnEnter() {
		const kind = model.completionKind
		if (kind === CompletionKind.shellCommand || kind === CompletionKind.shellPath || kind === CompletionKind.shellArg)
			return popup.explicitlySelected
		if (kind === CompletionKind.argument) {
			const cmd = model.completionCommand
			if (cmd === "cd" || cmd === "export")
				return popup.explicitlySelected
			if (model.completionQuery === "" && !popup.explicitlySelected)
				return false
			return true
		}
		return true
	}

	// Takes the entry selected in the list. Enter on a command that needs no arguments, or that is typed in
	// full, runs it; otherwise the command is completed.
	function acceptCompletion(viaEnter) {
		if (model.completionKind === CompletionKind.file) {
			model.acceptFile(popup.currentIndex)
			return
		}
		if (model.completionKind === CompletionKind.command) {
			const entry = popup.commandAt(popup.currentIndex)
			if (!entry)
				return
			const requiresArgs = Boolean(entry.requiresArgs)
			const typedWord = model.completionQuery.toLowerCase()
			const names = [entry.slash, ...(entry.aliases ?? [])].map(name => name.toLowerCase())
			const typedInFull = names.includes(typedWord)
			const standalone = model.isStandaloneCommandTarget()
			model.completeCommand(entry.slash, requiresArgs)
			if (viaEnter && standalone && (!requiresArgs || typedInFull))
				submit()
			return
		}
		if (model.acceptCompletionItem(popup.currentIndex, viaEnter))
			submit()
	}

	function syncFromModel() {
		input.syncing = true
		if (input.text !== model.text)
			input.text = model.text
		if (input.cursorPosition !== model.cursor)
			input.cursorPosition = model.cursor
		input.lastCursor = input.cursorPosition
		input.preSnapCursor = -1
		input.syncing = false
	}
	readonly property real queueBlock: queueStrip.height > 0
		? queueStrip.height + queueGap * Math.min(1, queueStrip.height / queueStrip.queueRowHeight) * Math.min(1, box.height / Math.max(1, queueGap))
		: 0

	implicitHeight: queueBlock + box.height
	height: implicitHeight
	onModelChanged: syncFromModel()

	Component.onCompleted: {
		model.interruptWindow = Motion.interruptWindow
		syncFromModel()
		ready = true
	}
	Connections {
		target: composer.model
		function onDraftReplaced() { composer.syncFromModel() }
		function onUserEdited() { popup.resetSelection() }
	}

	QueueStrip {
		id: queueStrip
		z: 1
		anchors { left: parent.left; right: parent.right; top: parent.top }
		model: composer.model.queue
		onWithdrawRequested: row => composer.model.withdraw(row)
	}
	Rectangle {
		id: box

		readonly property real attachmentBlock: strip.count > 0 ? strip.implicitHeight + composer.thumbRowGap : 0
		readonly property bool dropping: drop.containsDrag
		readonly property real fullHeight: composer.padding + attachmentBlock + Math.min(input.implicitHeight, composer.maxInputHeight)
			+ composer.textGap + composer.tagRowHeight + composer.padding

		anchors { left: parent.left; right: parent.right }
		y: composer.queueBlock
		height: composer.boxShown ? fullHeight : 0
		radius: Theme.radiusLarge
		color: composer.model.shellMode ? Theme.shellSurface : Theme.surface
		border.color: composer.model.shellMode ? Theme.shellBorder
			: (dropping || input.activeFocus ? Theme.accentText : Theme.border)
		clip: true
		opacity: composer.boxShown ? 1 : 0
		visible: composer.boxShown || height > 0
		enabled: composer.boxShown

		Behavior on opacity {
			enabled: composer.ready
			NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
		}
		Behavior on height {
			enabled: composer.ready
			NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
		}
		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		AttachmentStrip {
			id: strip
			anchors { left: parent.left; right: parent.right; top: parent.top; margins: composer.padding }
			model: composer.model.attachments
			visible: box.attachmentBlock > 0
			onRemoveRequested: row => composer.model.removeAttachment(row)
		}

		Basic.ScrollView {
			anchors {
				left: parent.left
				right: parent.right
				bottom: tags.top
				bottomMargin: composer.textGap
				leftMargin: composer.padding + composer.inputLeftInset
				rightMargin: composer.padding
			}
			height: Math.min(input.implicitHeight, composer.maxInputHeight)

			Basic.TextArea {
				id: input
				property bool syncing: false
				property bool pasteMenuEnabled: false
				property int lastCursor: 0
				property int preSnapCursor: -1
				property int preSnapPrev: 0
				property int snappedTarget: -1
				placeholderText: !composer.session.ready ? qsTr("Waiting for the ref backend…")
					: composer.model.shellMode ? qsTr("Run a shell command…")
					: composer.session.busy ? qsTr("Type to queue a message…")
					: qsTr("Message ref")
				placeholderTextColor: Theme.textMuted
				wrapMode: Basic.TextArea.Wrap
				// What is written back (history, undo, a withdrawn prompt) is text, whatever tags it holds.
				textFormat: TextEdit.PlainText
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font.pixelSize: Theme.fontSize
				background: null
				Component.onCompleted: TextEditSupport.disableUndoRedo(input.textDocument)
				TextEdit {
					id: tokenOverlay
					objectName: "composerTokenOverlay"
					anchors.fill: parent
					z: 1
					enabled: false
					readOnly: true
					focus: false
					activeFocusOnTab: false
					textFormat: TextEdit.PlainText
					wrapMode: input.wrapMode
					font: input.font
					color: "transparent"
					leftPadding: input.leftPadding
					rightPadding: input.rightPadding
					topPadding: input.topPadding
					bottomPadding: input.bottomPadding
					horizontalAlignment: input.horizontalAlignment
					verticalAlignment: input.verticalAlignment
					visible: input.length > 0 || input.inputMethodComposing
					text: input.inputMethodComposing
						? input.text.slice(0, input.cursorPosition) + input.preeditText + input.text.slice(input.cursorPosition)
						: input.text

					LinkColorizer {
						document: tokenOverlay.textDocument
						color: Theme.accentText
						highlightImageTokens: true
					}
				}
				Basic.ContextMenu.menu: Basic.Menu {
					id: editMenu
					objectName: "composerContextMenu"
					font.pixelSize: Math.round(Theme.secondaryFontSize * UiScale.factor)
					palette.window: Theme.surface
					palette.windowText: Theme.text
					palette.dark: Theme.border
					palette.mid: Theme.border
					palette.light: Theme.surfaceHover
					palette.midlight: Theme.surfaceHover
					palette.disabled.windowText: Theme.textMuted
					onAboutToShow: input.pasteMenuEnabled = composer.model.canPaste()
					Basic.MenuItem {
						objectName: "composerMenuUndo"
						text: qsTr("Undo")
						enabled: composer.model.canUndo
						onTriggered: composer.model.undo()
					}
					Basic.MenuItem {
						objectName: "composerMenuRedo"
						text: qsTr("Redo")
						enabled: composer.model.canRedo
						onTriggered: composer.model.redo()
					}
					Basic.MenuSeparator {}
					Basic.MenuItem {
						objectName: "composerMenuCut"
						text: qsTr("Cut")
						enabled: !input.readOnly && input.selectedText.length > 0
						onTriggered: input.cut()
					}
					Basic.MenuItem {
						objectName: "composerMenuCopy"
						text: qsTr("Copy")
						enabled: input.selectedText.length > 0
						onTriggered: input.copy()
					}
					Basic.MenuItem {
						objectName: "composerMenuPaste"
						text: qsTr("Paste")
						enabled: !input.readOnly && input.pasteMenuEnabled
						onTriggered: {
							if (!composer.model.pasteFromClipboard())
								input.paste()
						}
					}
					Basic.MenuItem {
						objectName: "composerMenuDelete"
						text: qsTr("Delete")
						enabled: !input.readOnly && input.selectedText.length > 0
						onTriggered: input.remove(input.selectionStart, input.selectionEnd)
					}
					Basic.MenuSeparator {}
					Basic.MenuItem {
						objectName: "composerMenuSelectAll"
						text: qsTr("Select All")
						enabled: input.length > 0
						onTriggered: input.selectAll()
					}
				}
				function syncSelectionBounds() {
					if (syncing || inputMethodComposing || selectionStart === selectionEnd)
						return
					const backward = cursorPosition === selectionStart
					const expanded = composer.model.expandSelection(selectionStart, selectionEnd)
					if (!expanded.changed)
						return
					syncing = true
					if (backward)
						select(expanded.end, expanded.start)
					else
						select(expanded.start, expanded.end)
					syncing = false
					lastCursor = cursorPosition
					composer.model.moveCursor(cursorPosition)
				}
				onTextChanged: {
					if (syncing)
						return
					if (preSnapCursor >= 0 && cursorPosition === snappedTarget) {
						syncing = true
						cursorPosition = preSnapCursor
						syncing = false
						lastCursor = preSnapPrev
					}
					preSnapCursor = -1
					composer.model.edit(text, cursorPosition)
					if (selectionStart === selectionEnd) {
						const snapped = composer.model.snapCursor(cursorPosition, lastCursor)
						if (snapped !== cursorPosition) {
							syncing = true
							cursorPosition = snapped
							syncing = false
							composer.model.moveCursor(cursorPosition)
						}
					}
					lastCursor = cursorPosition
				}
				onCursorPositionChanged: {
					if (syncing || inputMethodComposing)
						return
					if (selectionStart === selectionEnd) {
						const snapped = composer.model.snapCursor(cursorPosition, lastCursor)
						if (snapped !== cursorPosition) {
							preSnapCursor = cursorPosition
							preSnapPrev = lastCursor
							snappedTarget = snapped
							syncing = true
							cursorPosition = snapped
							syncing = false
						} else {
							preSnapCursor = -1
						}
					} else {
						preSnapCursor = -1
					}
					lastCursor = cursorPosition
					if (text === composer.model.text)
						composer.model.moveCursor(cursorPosition)
				}
				onSelectionStartChanged: syncSelectionBounds()
				onSelectionEndChanged: syncSelectionBounds()
				// The handlers below return true when the key is theirs, so that the first one that owns the key ends
				// the search (whether or not it accepts the event).

				// Enter sends, accepts a completion, or (with Shift) is left to the text area; Ctrl+J breaks the line.
				function handleEnter(event, mods) {
					if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
						if (mods.shift)
							return true
						event.accepted = true
						if (popup.open && composer.shouldAcceptOnEnter())
							composer.acceptCompletion(true)
						else
							composer.submit()
						return true
					}
					if (event.key === Qt.Key_J && mods.ctrl && !mods.shift) {
						event.accepted = true
						input.insert(input.cursorPosition, "\n")
						return true
					}
					return false
				}

				// Tab takes a completion, triggers shell completion in `!` mode, or switches the agent; Shift+Tab moves back.
				function handleTab(event, mods) {
					if (event.key === Qt.Key_Tab && mods.plain) {
						event.accepted = true
						if (popup.open)
							composer.acceptCompletion(false)
						else if (composer.model.shellMode)
							composer.model.triggerCompletion()
						else
							Workspace.cycleAgent(composer.session.sessionId, 1)
						return true
					}
					if (event.key === Qt.Key_Backtab || (event.key === Qt.Key_Tab && mods.shift)) {
						event.accepted = true
						if (composer.model.shellMode) {
							if (popup.open)
								popup.move(-1)
							else
								composer.model.triggerCompletion()
						} else if (popup.open) {
							popup.move(-1)
						} else {
							Workspace.cycleAgent(composer.session.sessionId, -1)
						}
						return true
					}
					return false
				}

				// Up, Down, Ctrl+P and Ctrl+N move through an open completion list.
				function handleCompletionMove(event, mods) {
					if (!popup.open)
						return false
					const movesUp = (event.key === Qt.Key_Up && mods.plain) || (event.key === Qt.Key_P && mods.ctrl)
					const movesDown = (event.key === Qt.Key_Down && mods.plain) || (event.key === Qt.Key_N && mods.ctrl)
					if (movesUp || movesDown) {
						event.accepted = true
						popup.move(movesUp ? -1 : 1)
					}
					return movesUp || movesDown
				}

				// Up and Down walk the history of sent messages.
				function handleHistory(event, mods) {
					if (!mods.plain)
						return false
					if (event.key === Qt.Key_Up) {
						event.accepted = composer.model.historyPrevious()
						return true
					}
					if (event.key === Qt.Key_Down) {
						event.accepted = composer.model.historyNext()
						return true
					}
					return false
				}

				// Esc closes what is open, else cancels a delayed tool call.
				function handleEscape(event) {
					if (event.key !== Qt.Key_Escape)
						return false
					const nothingToClose = !popup.open && !composer.model.shellMode
					if (nothingToClose && composer.session.lastToolDelayed) {
						event.accepted = true
						composer.session.cancelDelayedTool()
						Workspace.showToast(ToastLevel.info, qsTr("Cancelled the delayed tool call"))
					} else {
						event.accepted = composer.model.escape()
					}
					return true
				}

				// Undo/Redo use the model's stack so attachments stay in sync; Backspace leaves shell mode; Copy without a selection clears the draft; Paste may take an image.
				function handleEditing(event) {
					if (event.matches(StandardKey.Undo)) {
						event.accepted = true
						composer.model.undo()
						return true
					}
					if (event.matches(StandardKey.Redo)) {
						event.accepted = true
						composer.model.redo()
						return true
					}
					if (event.key === Qt.Key_Backspace && composer.model.shellMode && input.length === 0) {
						event.accepted = true
						composer.model.leaveShell()
						return true
					}
					if ((event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) && input.selectionStart === input.selectionEnd) {
						if (composer.model.deletePlaceholderAt(input.cursorPosition, event.key === Qt.Key_Backspace)) {
							event.accepted = true
							return true
						}
					}
					if (event.matches(StandardKey.Copy) && input.selectedText === "") {
						event.accepted = true
						composer.model.clearDraft()
						return true
					}
					if (event.matches(StandardKey.Paste)) {
						event.accepted = composer.model.pasteFromClipboard()
						return true
					}
					return false
				}

				Keys.onPressed: event => {
					const composingPreedit = input.inputMethodComposing && input.preeditText !== ""
					if (composingPreedit) {
						if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
							event.accepted = true
						return
					}
					input.preSnapCursor = -1
					composer.keyOffered(event)
					if (event.accepted)
						return
					const shift = (event.modifiers & Qt.ShiftModifier) !== 0
					const ctrl = (event.modifiers & Qt.ControlModifier) !== 0
					const mods = {shift: shift, ctrl: ctrl, plain: !shift && !ctrl && (event.modifiers & ~Qt.KeypadModifier) === 0}
					handleEnter(event, mods) || handleTab(event, mods) || handleCompletionMove(event, mods) || handleHistory(event, mods)
						|| handleEscape(event) || handleEditing(event)
				}
			}
		}

		ComposerTags {
			id: tags
			model: composer.model
			anchors {
				left: parent.left
				leftMargin: composer.padding
				right: sendButton.left
				rightMargin: composer.padding
				bottom: parent.bottom
				bottomMargin: composer.padding
			}
			height: composer.tagRowHeight
			onAgentClicked: Commands.run(CommandId.agents, "", composer.session.sessionId)
			onModelClicked: Commands.run(CommandId.models, "", composer.session.sessionId)
			onVariantClicked: Workspace.openDialog(DialogId.variants, {sessionId: composer.session.sessionId})
		}

		SendButton {
			id: sendButton
			anchors { right: parent.right; bottom: parent.bottom; margins: composer.padding }
			busy: composer.session.busy && !composer.model.hasContent
			canSend: composer.model.hasContent && composer.session.ready
			activeColor: composer.model.shellMode ? Theme.shellBorder : Theme.accent
			onClicked: busy ? composer.session.stop() : composer.submit()
		}

		DropArea {
			id: drop
			anchors.fill: parent
			onDropped: event => {
				if (event.hasUrls)
					composer.model.attachUrls(event.urls)
			}
		}

		Rectangle {
			anchors.fill: parent
			radius: box.radius
			color: Theme.surface
			opacity: box.dropping ? 0.9 : 0
			visible: opacity > 0

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			Text {
				textFormat: Text.PlainText
				anchors.centerIn: parent
				text: qsTr("Drop files here")
				color: Theme.textMuted
				font.pixelSize: Theme.fontSize
			}
		}
	}

	CompletionPopup {
		id: popup
		z: 2
		anchors { left: parent.left; right: parent.right; bottom: parent.top; bottomMargin: popupGap }
		kind: composer.model.completionKind
		fileModel: composer.model.files
		itemModel: composer.model.completions
		entries: composer.model.completionKind === CompletionKind.command ? Commands.matching(composer.model.completionQuery, true) : []
		onAccepted: index => {
			popup.currentIndex = index
			composer.acceptCompletion(false)
			composer.focusInput()
		}
	}
}
