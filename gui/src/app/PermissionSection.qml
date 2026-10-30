import QtQuick
import RefGui

// The question a tool card asks when its call needs permission: what it would run or touch,
// and the three answers. Opens under the card's details and closes once answered.
Item {
	id: section

	required property string toolName
	required property var input
	required property bool open
	required property int othersWaiting

	signal answered(bool allow, bool always)

	readonly property int padding: 14
	readonly property int fieldPadding: 8
	readonly property int fieldPaddingWide: 10
	readonly property int rowSpacing: 8
	readonly property string command: typeof input.command === "string" ? input.command : ""
	readonly property string path: typeof input.path === "string" ? input.path : ""

	width: parent.width
	height: open ? content.y + content.height : 0
	clip: true

	Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Rectangle {
		width: parent.width
		height: 1
		color: Theme.border
	}

	Column {
		id: content
		x: section.padding
		y: 1 + section.rowSpacing + 2
		width: parent.width - 2 * section.padding
		bottomPadding: section.padding - 4
		spacing: section.rowSpacing

		Rectangle {
			visible: section.command !== ""
			width: parent.width
			height: commandText.height + 2 * section.fieldPadding
			radius: Theme.radiusSmall
			color: Theme.codeBackground

			Text {
				id: commandText
				x: section.fieldPaddingWide
				y: section.fieldPadding
				width: parent.width - 2 * section.fieldPaddingWide
				text: "$ " + section.command
				color: Theme.text
				font.family: Theme.monoFamily
				font.pixelSize: Theme.codeFontSize
				font.bold: true
				wrapMode: Text.Wrap
			}
		}

		Text {
			visible: section.path !== ""
			width: parent.width
			text: section.path
			color: Theme.info
			font.family: Theme.monoFamily
			font.pixelSize: Theme.codeFontSize
			elide: Text.ElideMiddle
		}

		Item {
			width: parent.width
			height: allow.height

			Row {
				spacing: section.rowSpacing

				PermissionButton {
					id: allow
					iconName: "check"
					label: qsTr("Allow")
					keyName: "a"
					tint: Theme.success
					onClicked: section.answered(true, false)
				}
				PermissionButton {
					iconName: "check-all"
					label: qsTr("Allow for session")
					keyName: "s"
					tint: Theme.accent
					onClicked: section.answered(true, true)
				}
				PermissionButton {
					iconName: "close"
					label: qsTr("Deny")
					keyName: "d"
					tint: Theme.danger
					onClicked: section.answered(false, false)
				}
			}

			Text {
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				visible: section.othersWaiting > 0
				text: qsTr("+%1 more waiting").arg(section.othersWaiting)
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}
		}
	}
}
