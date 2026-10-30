// Adapted from an early version of qutie by LightBlueCube
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
		if (model.completionKind === "file") {
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

		anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
		height: composer.padding + attachmentBlock + Math.min(input.implicitHeight, composer.maxInputHeight)
			+ composer.textGap + composer.tagRowHeight + composer.padding
		radius: Theme.radiusLarge
		color: composer.model.shellMode ? Theme.shellSurface : Theme.surface
		border.color: composer.model.shellMode ? Theme.shellBorder
			: (dropping || input.activeFocus ? Theme.accent : Theme.border)
		clip: true

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
			id: scroll
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

				Keys.onPressed: event => {
					if (input.inputMethodComposing)
						return
					const shift = (event.modifiers & Qt.ShiftModifier) !== 0
					const ctrl = (event.modifiers & Qt.ControlModifier) !== 0
					const plain = !shift && !ctrl && (event.modifiers & ~Qt.KeypadModifier) === 0
					const listOpen = popup.open

					if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
						if (shift)
							return
						event.accepted = true
						if (listOpen)
							composer.acceptCompletion(true)
						else
							composer.submit()
					} else if (event.key === Qt.Key_J && ctrl && !shift) {
						event.accepted = true
						input.insert(input.cursorPosition, "\n")
					} else if (event.key === Qt.Key_Tab && plain) {
						event.accepted = true
						if (listOpen)
							composer.acceptCompletion(false)
						else
							Workspace.cycleAgent(composer.session.sessionId, 1)
					} else if (event.key === Qt.Key_Backtab) {
						event.accepted = true
						Workspace.cycleAgent(composer.session.sessionId, -1)
					} else if (listOpen && (event.key === Qt.Key_Up && plain || event.key === Qt.Key_P && ctrl)) {
						event.accepted = true
						popup.move(-1)
					} else if (listOpen && (event.key === Qt.Key_Down && plain || event.key === Qt.Key_N && ctrl)) {
						event.accepted = true
						popup.move(1)
					} else if (event.key === Qt.Key_Up && plain) {
						event.accepted = composer.model.historyPrevious()
					} else if (event.key === Qt.Key_Down && plain) {
						event.accepted = composer.model.historyNext()
					} else if (event.key === Qt.Key_Escape) {
						const nothingToClose = composer.model.completionKind === "none" && !composer.model.shellMode
						if (nothingToClose && composer.session.lastToolDelayed) {
							event.accepted = true
							composer.session.cancelDelayedTool()
							Workspace.toastRequested("info", qsTr("Cancelled the delayed tool call"))
						} else {
							event.accepted = composer.model.escape()
						}
					} else if (event.key === Qt.Key_Backspace && composer.model.shellMode && input.length === 0) {
						event.accepted = true
						composer.model.leaveShell()
					} else if (event.matches(StandardKey.Copy) && input.selectedText === "") {
						event.accepted = true
						composer.model.clearDraft()
					} else if (event.matches(StandardKey.Paste)) {
						event.accepted = composer.model.pasteFromClipboard()
					}
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
			onAgentClicked: Commands.run("agents", "", composer.session.sessionId)
			onModelClicked: Commands.run("models", "", composer.session.sessionId)
			onVariantClicked: Workspace.openDialog("variants", {sessionId: composer.session.sessionId})
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
		entries: composer.model.completionKind === "file" ? composer.model.files
			: (composer.model.completionKind === "command" ? Commands.match(composer.model.completionQuery, true) : [])
		onAccepted: index => {
			popup.currentIndex = index
			composer.acceptCompletion(false)
			composer.focusInput()
		}
	}
}
