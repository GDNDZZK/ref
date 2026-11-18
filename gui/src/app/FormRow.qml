import QtQuick
import RefGui

// Labeled configuration row. Horizontal by default (label + description on the left, control on
// the right); set `stacked: true` for wide or multi-line editors placed below the header.
Item {
	id: row

	property string label: ""
	property string description: ""
	property string badge: ""
	property bool stacked: false
	// Width of the right-hand control slot in horizontal mode; <= 0 sizes to the child's own width.
	property int controlWidth: 280
	property bool showReset: false
	property string resetText: qsTr("Reset")
	property bool showDivider: false

	default property alias content: controlSlot.data

	signal resetClicked()

	readonly property real resolvedControlWidth: {
		if (stacked)
			return width
		if (controlWidth > 0)
			return Math.min(controlWidth, Math.max(120, Math.floor(width * 0.58)))
		return controlSlot.childrenRect.width
	}
	readonly property real headerBlockHeight: titleRow.implicitHeight
		+ (descriptionLabel.visible ? 4 + descriptionLabel.implicitHeight : 0)
	readonly property real controlBlockHeight: controlSlot.childrenRect.height

	width: parent ? parent.width : 480
	implicitHeight: stacked
		? headerBlockHeight + (controlBlockHeight > 0 ? 10 + controlBlockHeight : 0) + 20
		: Math.max(headerBlockHeight, controlBlockHeight) + 20

	Item {
		id: labelColumn
		anchors {
			left: parent.left
			right: row.stacked ? (resetButton.visible ? resetButton.left : parent.right) : controlSlot.left
			rightMargin: row.stacked ? (resetButton.visible ? 12 : 0) : 16
			top: row.stacked ? parent.top : undefined
			topMargin: row.stacked ? 10 : 0
			verticalCenter: row.stacked ? undefined : parent.verticalCenter
		}
		height: row.headerBlockHeight

		Item {
			id: titleRow
			width: parent.width
			implicitHeight: Math.max(labelText.implicitHeight, badgePill.visible ? badgePill.height : 0)
			height: implicitHeight

			Text {
				id: labelText
				textFormat: Text.PlainText
				anchors { left: parent.left; verticalCenter: parent.verticalCenter }
				width: Math.min(implicitWidth, Math.max(0, parent.width - (badgePill.visible ? badgePill.width + 8 : 0)))
				text: row.label
				color: Theme.text
				font.pixelSize: Theme.secondaryFontSize
				elide: Text.ElideRight
			}

			Rectangle {
				id: badgePill
				visible: row.badge !== ""
				anchors { left: labelText.right; leftMargin: 8; verticalCenter: parent.verticalCenter }
				width: Math.min(badgeText.implicitWidth + 10, Math.max(40, Math.floor(parent.width * 0.4)))
				height: 18
				radius: 4
				color: Theme.codeBackground
				border.color: Theme.border

				Text {
					id: badgeText
					textFormat: Text.PlainText
					anchors { left: parent.left; right: parent.right; leftMargin: 5; rightMargin: 5; verticalCenter: parent.verticalCenter }
					text: row.badge
					color: Theme.textMuted
					font.pixelSize: Theme.microFontSize
					elide: Text.ElideRight
				}
			}
		}

		Text {
			id: descriptionLabel
			textFormat: Text.PlainText
			visible: row.description !== ""
			anchors { top: titleRow.bottom; topMargin: 4; left: parent.left; right: parent.right }
			text: row.description
			color: Theme.textMuted
			font.pixelSize: Theme.smallFontSize
			wrapMode: Text.Wrap
		}
	}

	TextButton {
		id: resetButton
		visible: row.showReset
		anchors {
			right: row.stacked ? parent.right : controlSlot.left
			rightMargin: row.stacked ? 0 : 8
			top: row.stacked ? parent.top : undefined
			topMargin: row.stacked ? 8 : 0
			verticalCenter: row.stacked ? undefined : parent.verticalCenter
		}
		text: row.resetText
		onClicked: row.resetClicked()
	}

	Item {
		id: controlSlot
		anchors {
			right: parent.right
			left: row.stacked ? parent.left : undefined
			top: row.stacked ? labelColumn.bottom : undefined
			topMargin: row.stacked ? 10 : 0
			verticalCenter: row.stacked ? undefined : parent.verticalCenter
		}
		width: row.stacked ? parent.width : row.resolvedControlWidth
		height: childrenRect.height
	}

	Rectangle {
		visible: row.showDivider
		anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
		height: 1
		color: Theme.border
	}
}
