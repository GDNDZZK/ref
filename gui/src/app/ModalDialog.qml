import QtQuick
import QtQuick.Controls
import RefGui

// The frame of every dialog: panel with a title, the content (which scrolls when it is too
// tall) and an optional row of buttons. The scrim is the DialogHost's. Esc and a click on the
// scrim close it; the panel drops in and fades in on creation, and fades out when closed.
FocusScope {
	id: dialog

	property string title: ""
	property int dialogWidth: Theme.dialogWidth
	default property alias content: body.data
	property alias buttons: footerRow.data
	// Content beyond this height scrolls.
	readonly property real maxPanelHeight: height * Theme.dialogMaxHeightRatio
	readonly property bool hasButtons: footerRow.children.length > 0
	readonly property bool open: Workspace.dialog !== ""
	property bool entered: false
	// Set when the dialog is destroyed, so a reply that arrives later does nothing.
	property bool gone: false

	signal accepted()

	function close() {
		Workspace.closeDialog()
	}

	// Wraps the callback of a backend request so that it is skipped once the dialog is gone.
	function replyWhileOpen(handler) {
		return (...reply) => {
			if (!gone)
				handler(...reply)
		}
	}

	// A reply callback (ok, reason) that closes the dialog on success and puts the reason under the field otherwise.
	function closeOrShowError(field) {
		return replyWhileOpen((ok, reason) => {
			if (ok)
				close()
			else
				field.errorText = reason
		})
	}

	anchors.fill: parent
	focus: true

	Component.onCompleted: {
		entered = true
		forceActiveFocus()
	}

	Component.onDestruction: gone = true

	Keys.onEscapePressed: close()
	Keys.onReturnPressed: accepted()
	Keys.onEnterPressed: accepted()

	// A click outside the panel closes the dialog.
	TapHandler {
		onTapped: dialog.close()
	}

	Rectangle {
		id: panel

		readonly property real wantedY: Math.max(Theme.overlayEdgeMargin, (dialog.height - height) / 3)
		readonly property bool shown: dialog.entered && dialog.open

		width: Math.min(dialog.dialogWidth, dialog.width - 2 * Theme.overlayEdgeMargin)
		height: Math.min(dialog.maxPanelHeight, Theme.dialogHeaderHeight + scroller.contentHeight + (dialog.hasButtons ? Theme.dialogFooterHeight : 0))
		x: Math.round((dialog.width - width) / 2)
		y: Math.round(wantedY) - (shown ? 0 : Motion.dialogDrop)
		radius: Theme.radiusLarge
		color: Theme.surface
		border.color: Theme.border
		clip: true
		opacity: shown ? 1 : 0
		scale: shown ? 1 : Motion.popScale

		Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on opacity { NumberAnimation { duration: panel.shown ? Motion.standard : Motion.quick; easing.type: Motion.ease } }
		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		// Swallows clicks, so the dialog's own tap handler only sees the ones outside.
		TapHandler { gesturePolicy: TapHandler.ReleaseWithinBounds }

		Item {
			id: header
			width: parent.width
			height: Theme.dialogHeaderHeight

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: closeButton.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: dialog.title
				color: Theme.text
				font { pixelSize: Theme.fontSize + 1; bold: true }
				elide: Text.ElideRight
			}

			IconButton {
				id: closeButton
				anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
				buttonSize: 28
				iconName: "close"
				Accessible.name: qsTr("Close")
				onClicked: dialog.close()
			}
		}

		Flickable {
			id: scroller
			anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: footer.top }
			contentWidth: width
			contentHeight: body.implicitHeight
			boundsBehavior: Flickable.StopAtBounds
			clip: true

			ScrollBar.vertical: ThemedScrollBar {}

			Item {
				id: body
				width: scroller.width
				implicitHeight: childrenRect.height
				height: implicitHeight
			}
		}

		Item {
			id: footer
			anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
			height: dialog.hasButtons ? Theme.dialogFooterHeight : 0

			Row {
				id: footerRow
				anchors { right: parent.right; rightMargin: Theme.dialogPadding; verticalCenter: parent.verticalCenter }
				spacing: 10
			}
		}
	}
}
