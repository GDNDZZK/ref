// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// One session: its conversation, status, jump-to-latest button and input box. Takes the
// session it shows; the window's single view and every split-view tile are one of these.
Item {
	id: pane

	required property ChatSession session
	// In a split-view tile: tighter margins.
	property bool compact: false
	property bool layoutCompact: compact
	property real layoutWidth: width
	readonly property real compactMargin: 10 // below the input box
	readonly property real compactColumnPadding: 16 // beside the conversation
	readonly property real questionMaxHeightRatio: 0.8 // of the pane, for the question card
	readonly property real sendButtonFallbackInset: 24 // Composer.padding(8) + SendButton.width(32) / 2
	readonly property real margin: compact ? compactMargin : Theme.pagePadding
	readonly property real columnPadding: layoutCompact ? compactColumnPadding : Theme.pagePadding
	property bool keysEnabled: true
	// Whether the input box is there. A hidden one keeps its draft, attachments and queue;
	// the queue strip stays either way.
	property bool composerShown: true

	function focusInput() {
		if (composerLoader.item)
			(composerLoader.item as Composer).focusInput()
	}
	function isJumpButtonAt(scenePos) {
		if (!jumpToEnd.visible)
			return false
		const p = jumpToEnd.mapFromItem(null, scenePos.x, scenePos.y)
		return p.x >= 0 && p.y >= 0 && p.x <= jumpToEnd.width && p.y <= jumpToEnd.height
	}

	// The keyboard goes to the answer keys while they may answer, and back to the input box after.
	function updateKeyFocus() {
		if (session.question.editing) // the card's text field has it
			return
		if (keysEnabled && session.answerKeysActive)
			permissionKeys.forceActiveFocus()
		else if (permissionKeys.activeFocus)
			focusInput()
	}

	// The keys start with a new guard time when this pane gets them: the user may just have clicked here,
	// or typed elsewhere, and must not answer by accident.
	function armKeys() {
		if (keysEnabled && session)
			session.restartAnswerGuard()
	}

	onKeysEnabledChanged: {
		armKeys()
		updateKeyFocus()
	}
	onSessionChanged: armKeys()

	// The rows are deleted inside the destructor of the ChatView, when its QML part is already gone, and the rows'
	// bindings that read the view then run against a half-destroyed object (TypeErrors). Dropping the rows first, while
	// the view is whole, ends them in the normal way.
	Component.onDestruction: chat.model = null
	clip: true

	ChatView {
		id: chat
		anchors { left: parent.left; right: parent.right; top: parent.top; bottom: questionCard.top }
		model: pane.session.blocks
		compact: pane.compact
		layoutWidth: pane.layoutWidth
		sidePadding: pane.columnPadding
		sessionId: pane.session.sessionId
		session: pane.session
		permissionCount: pane.session.permissionCount
		moreBefore: pane.session.moreBefore
		loadingOlder: pane.session.loadingOlder
		olderError: pane.session.olderError
		onLoadOlderRequested: pane.session.loadOlder()
		onImageClicked: image => {
			const list = pane.session.allImages()
			lightboxLoader.pendingSessionId = pane.session.sessionId
			lightboxLoader.pendingImage = image
			lightboxLoader.pendingImages = list
			if (lightboxLoader.item)
				(lightboxLoader.item as Lightbox).open(pane.session.sessionId, image, list)
			else
				lightboxLoader.active = true
		}
		onCancelRequested: pane.session.cancelDelayedTool()
		onUndoRequested: messageId => pane.session.undo(messageId)
		onCopyRequested: messageId => pane.session.copyMessage(messageId)
		onPermissionAnswered: (callId, allow, always) => pane.session.permissionReply(callId, allow, always)
	}

	Connections {
		target: pane.session

		function onAboutToMaterialize() {
			chat.prepareMaterialize()
		}
		function onMaterialized() {
			chat.finishMaterialize()
		}
	}

	// Takes the keyboard from the input box once the keys may answer a permission request
	// (the user may be typing when it appears), and hands it back when none is left. The input
	// box also offers its keys (onKeyOffered), so a click into it does not turn them off.
	Item {
		id: permissionKeys

		Keys.onPressed: event => {
			pane.session.answerKey(event.key, event.text, event.modifiers, event.isAutoRepeat)
			event.accepted = true
		}

		Connections {
			target: pane.session

			function onAnswerChanged() {
				pane.updateKeyFocus()
			}
		}
	}

	// This session's last failure, until it is gone. Connection trouble, which concerns all
	// sessions, is the window's (Main).
	StatusPill {
		text: Workspace.connectionText === "" ? pane.session.statusText : ""
		actionText: Workspace.connectionText === "" && pane.session.rangeError !== "" ? qsTr("Retry") : ""
		onActionClicked: pane.session.retryRangeFetch()
	}

	Text {
		textFormat: Text.PlainText
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
		x: composerLoader.x + composerLoader.sendButtonCenterX - width / 2
		anchors { bottom: questionCard.top; bottomMargin: 4 }
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

	QuestionCard {
		id: questionCard
		anchors { horizontalCenter: parent.horizontalCenter; bottom: statusLine.top }
		width: chat.columnWidth
		prompt: pane.session.question
		maxHeight: Math.max(0, Math.min(pane.height * pane.questionMaxHeightRatio, pane.height - statusLine.height - composerLoader.height - pane.margin))
	}

	StatusLine {
		id: statusLine
		session: pane.session
		anchors { horizontalCenter: parent.horizontalCenter; bottom: composerLoader.top }
		width: chat.columnWidth
		onTodoDismissed: pane.focusInput()
	}

	Loader {
		id: composerLoader
		objectName: "composerLoader"
		anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: Math.min(pane.margin, composerLoader.height) }
		width: chat.columnWidth
		active: true
		readonly property real sendButtonCenterX: item ? (item as Composer).sendButtonCenterX : width - pane.sendButtonFallbackInset
		sourceComponent: Composer {
			width: composerLoader.width
			session: pane.session
			boxShown: pane.composerShown
			onSubmitted: chat.following = true
			// With a request pending, its keys come before the input box, wherever the caret is.
			onKeyOffered: event => event.accepted = pane.keysEnabled && pane.session.answerKey(event.key, event.text, event.modifiers, event.isAutoRepeat)
		}
	}

	Loader {
		id: lightboxLoader
		anchors.fill: parent
		active: false
		property string pendingSessionId: ""
		property var pendingImage: null
		property var pendingImages: []

		sourceComponent: Lightbox {}
		onLoaded: {
			if (pendingImage)
				(item as Lightbox).open(pendingSessionId, pendingImage, pendingImages)
		}
		Connections {
			target: lightboxLoader.item
			function onClosed() { pane.focusInput() }
			function onVisibleChanged() {
				if (lightboxLoader.item && !lightboxLoader.item.visible && !lightboxLoader.item.shown)
					lightboxLoader.active = false
			}
		}
	}

	Component.onCompleted: {
		armKeys()
		if (composerShown)
			focusInput()
	}
}
