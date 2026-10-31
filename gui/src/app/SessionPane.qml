// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// One session: its conversation, status, jump-to-latest button and input box. Takes the
// session it shows; the window's single view and every split-view tile are one of these.
Item {
	id: pane

	required property ChatSession session
	// In a split-view tile: tighter margins.
	property bool compact: false
	readonly property real compactMargin: 10 // below the input box
	readonly property real compactColumnPadding: 16 // beside the conversation
	readonly property real margin: compact ? compactMargin : Theme.pagePadding
	readonly property real columnPadding: compact ? compactColumnPadding : Theme.pagePadding
	// Whether the keys may answer a permission request of this session (one pane at a time has them).
	property bool keysEnabled: true
	// Whether the input box is there. A hidden one keeps its draft, attachments and queue;
	// the queue strip stays either way.
	property bool composerShown: true

	function focusInput() {
		composer.focusInput()
	}

	// The keyboard goes to the permission keys while they may answer, and back to the input box after.
	function updateKeyFocus() {
		if (keysEnabled && session.permissionKeysActive)
			permissionKeys.forceActiveFocus()
		else if (permissionKeys.activeFocus)
			focusInput()
	}

	onKeysEnabledChanged: updateKeyFocus()

	ChatView {
		id: chat
		anchors { left: parent.left; right: parent.right; top: parent.top; bottom: statusLine.top }
		model: pane.session.blocks
		sidePadding: pane.columnPadding
		turnInProgress: pane.session.busy
		sessionId: pane.session.sessionId
		permissionCount: pane.session.permissionCount
		moreBefore: pane.session.moreBefore
		loadingOlder: pane.session.loadingOlder
		olderError: pane.session.olderError
		onLoadOlderRequested: pane.session.loadOlder()
		onImageClicked: image => lightbox.open(pane.session.sessionId, image)
		onCancelRequested: pane.session.cancelDelayedTool()
		onUndoRequested: messageId => pane.session.undo(messageId)
		onCopyRequested: messageId => pane.session.copyMessage(messageId)
		onPermissionAnswered: (callId, allow, always) => pane.session.permissionReply(callId, allow, always)
	}

	// Takes the keyboard from the input box once the keys may answer a permission request
	// (the user may be typing when it appears), and hands it back when none is left. The input
	// box also offers its keys (onKeyOffered), so a click into it does not turn them off.
	Item {
		id: permissionKeys

		Keys.onPressed: event => {
			pane.session.permissionKey(event.key, event.text, event.modifiers, event.isAutoRepeat)
			event.accepted = true
		}

		Connections {
			target: pane.session

			function onToolsChanged() {
				pane.updateKeyFocus()
			}
		}
	}

	// This session's last failure, until it is gone. Connection trouble, which concerns all
	// sessions, is the window's (Main).
	StatusPill {
		text: Workspace.connectionText === "" ? pane.session.statusText : ""
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
		x: composer.x + composer.sendButtonCenterX - width / 2
		anchors { bottom: statusLine.top; bottomMargin: 4 }
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

	StatusLine {
		id: statusLine
		session: pane.session
		anchors { horizontalCenter: parent.horizontalCenter; bottom: composer.top }
		width: chat.columnWidth
	}

	Composer {
		id: composer
		anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: pane.margin }
		width: chat.columnWidth
		session: pane.session
		boxShown: pane.composerShown
		onSubmitted: chat.following = true
		// With a request pending, its keys come before the input box, wherever the caret is.
		onKeyOffered: event => event.accepted = pane.keysEnabled && pane.session.permissionKey(event.key, event.text, event.modifiers, event.isAutoRepeat)
	}

	// The input box's remarks are the window's toasts.
	Connections {
		target: pane.session.composer
		function onToastRequested(level, text) {
			Workspace.toastRequested(level, text)
		}
	}

	Lightbox {
		id: lightbox
		onClosed: pane.focusInput()
	}

	Component.onCompleted: focusInput()
}
