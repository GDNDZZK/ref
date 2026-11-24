// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// A small notice pill at the top center: connection trouble, the last failure.
// Fades out when `text` is empty.
Rectangle {
	id: pill

	property string text: ""
	property string actionText: ""
	readonly property bool shown: text !== ""
	readonly property int horizontalPadding: 14
	readonly property int verticalPadding: 6
	readonly property int actionVerticalPadding: 8
	readonly property int actionRightMargin: 6
	readonly property int cornerClearance: 80 // clear of the corner buttons
	readonly property int actionGap: 10
	readonly property real contentWidth: label.implicitWidth + (actionButton.visible ? actionButton.width + actionGap : 0)

	signal actionClicked()

	anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 14 }
	width: Math.min(contentWidth + 2 * horizontalPadding, parent.width - 2 * cornerClearance)
	height: Math.max(label.implicitHeight + 2 * verticalPadding, actionButton.visible ? actionButton.height + actionVerticalPadding : 0)
	radius: height / 2
	color: Theme.surface
	border.color: Theme.border
	opacity: shown ? 1 : 0
	visible: opacity > 0

	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Text {
		textFormat: Text.PlainText
		id: label
		anchors {
			left: parent.left
			leftMargin: pill.horizontalPadding
			right: actionButton.visible ? actionButton.left : parent.right
			rightMargin: actionButton.visible ? pill.actionGap : pill.horizontalPadding
			verticalCenter: parent.verticalCenter
		}
		text: pill.text
		elide: Text.ElideRight
		color: Theme.textMuted
		font.pixelSize: Theme.smallFontSize
	}

	DialogButton {
		id: actionButton
		objectName: "statusPillActionButton"
		anchors { right: parent.right; rightMargin: pill.actionRightMargin; verticalCenter: parent.verticalCenter }
		small: true
		visible: pill.actionText !== ""
		text: pill.actionText
		onClicked: pill.actionClicked()
	}
}
