// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import QtQuick.Controls.Basic
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

	// Takes the entry selected in the list. Enter on a command that needs no arguments, or that is typed in
	// full, runs it; otherwise the command is completed.
	function acceptCompletion(viaEnter) {
		if (model.completionKind === CompletionKind.file) {
			model.acceptFile(popup.currentIndex)
			return
		}
		const entry = popup.commandAt(popup.currentIndex)
		if (!entry)
			return
		const takesArgs = entry.hint !== ""
		const typedInFull = model.text.trim() === "/" + entry.slash
		model.completeCommand(entry.slash)
		if (viaEnter && (!takesArgs || typedInFull))
			submit()
	}

	function syncFromModel() {
		input.syncing = true
		if (input.text !== model.text)
			input.text = model.text
		if (input.cursorPosition !== model.cursor)
			input.cursorPosition = model.cursor
		input.syncing = false
	}

	readonly property real queueBlock: queueStrip.height > 0
		? queueStrip.height + queueGap * Math.min(1, queueStrip.height / queueStrip.queueRowHeight) : 0

	implicitHeight: queueBlock + box.height
	onModelChanged: syncFromModel()

	Component.onCompleted: {
		model.interruptWindow = Motion.interruptWindow
		syncFromModel()
	}

	Connections {
		target: composer.model
		function onDraftReplaced() { composer.syncFromModel() }
	}

	QueueStrip {
		id: queueStrip
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

		anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
		height: composer.boxShown ? fullHeight : 0
		radius: Theme.radiusLarge
		color: composer.model.shellMode ? Theme.shellSurface : Theme.surface
		border.color: composer.model.shellMode ? Theme.shellBorder
			: (dropping || input.activeFocus ? Theme.accent : Theme.border)
		clip: true
		opacity: composer.boxShown ? 1 : 0
		visible: composer.boxShown || height > 0
		enabled: composer.boxShown

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		AttachmentStrip {
			id: strip
			anchors { left: parent.left; right: parent.right; top: parent.top; margins: composer.padding }
			model: composer.model.attachments
			visible: box.attachmentBlock > 0
			onRemoveRequested: row => composer.model.removeAttachment(row)
		}

		ScrollView {
			anchors {
				left: parent.left
				right: parent.right
				bottom: tags.top
				bottomMargin: composer.textGap
				leftMargin: composer.padding + composer.inputLeftInset
				rightMargin: composer.padding
			}
			height: Math.min(input.implicitHeight, composer.maxInputHeight)

			TextArea {
				id: input

				property bool syncing: false

				placeholderText: !composer.session.ready ? qsTr("Waiting for the ref backend…")
					: composer.model.shellMode ? qsTr("Run a shell command…")
					: composer.session.busy ? qsTr("Type to queue a message…")
					: qsTr("Message ref")
				placeholderTextColor: Theme.textMuted
				wrapMode: TextArea.Wrap
				// What is written back (history, undo, a withdrawn prompt) is text, whatever tags it holds.
				textFormat: TextEdit.PlainText
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font.pixelSize: Theme.fontSize
				background: null

				onTextChanged: if (!syncing) composer.model.edit(text, cursorPosition)
				onCursorPositionChanged: if (!syncing) composer.model.moveCursor(cursorPosition)

				// The handlers below return true when the key is theirs, so that the first one that owns the key ends
				// the search (whether or not it accepts the event).

				// Enter sends, accepts a completion, or (with Shift) is left to the text area; Ctrl+J breaks the line.
				function handleEnter(event, mods) {
					if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
						if (mods.shift)
							return true
						event.accepted = true
						if (popup.open)
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

				// Tab takes a completion, else switches the agent; Shift+Tab switches back.
				function handleTab(event, mods) {
					if (event.key === Qt.Key_Tab && mods.plain) {
						event.accepted = true
						if (popup.open)
							composer.acceptCompletion(false)
						else
							Workspace.cycleAgent(composer.session.sessionId, 1)
						return true
					}
					if (event.key === Qt.Key_Backtab) {
						event.accepted = true
						Workspace.cycleAgent(composer.session.sessionId, -1)
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
					const nothingToClose = composer.model.completionKind === CompletionKind.none && !composer.model.shellMode
					if (nothingToClose && composer.session.lastToolDelayed) {
						event.accepted = true
						composer.session.cancelDelayedTool()
						Workspace.showToast(ToastLevel.info, qsTr("Cancelled the delayed tool call"))
					} else {
						event.accepted = composer.model.escape()
					}
					return true
				}

				// Backspace leaves shell mode, Copy without a selection clears the draft, Paste may take an image.
				function handleEditing(event) {
					if (event.key === Qt.Key_Backspace && composer.model.shellMode && input.length === 0) {
						event.accepted = true
						composer.model.leaveShell()
						return true
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
					if (input.inputMethodComposing)
						return
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
				text: qsTr("Drop images to attach")
				color: Theme.textMuted
				font.pixelSize: Theme.fontSize
			}
		}
	}

	CompletionPopup {
		id: popup
		anchors { left: parent.left; right: parent.right; bottom: parent.top; bottomMargin: popupGap }
		kind: composer.model.completionKind
		entries: composer.model.completionKind === CompletionKind.file ? composer.model.files
			: (composer.model.completionKind === CompletionKind.command ? Commands.matching(composer.model.completionQuery, true) : [])
		onAccepted: index => {
			popup.currentIndex = index
			composer.acceptCompletion(false)
			composer.focusInput()
		}
	}
}
