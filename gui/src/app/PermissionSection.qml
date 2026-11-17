import QtQuick
import RefGui

// The question a tool card asks when its call needs permission: what it would run or touch,
// and the three answers. Opens under the card's details and closes once answered.
Item {
	id: section

	required property var input
	required property bool open
	required property int othersWaiting
	required property var preview

	signal answered(bool allow, bool always)

	readonly property int padding: 14
	readonly property int fieldPadding: 8
	readonly property int fieldPaddingWide: 10
	readonly property int rowSpacing: 8
	readonly property int dividerHeight: 1
	readonly property int topGap: 2 // extra room under the divider
	readonly property int bottomTrim: 4 // of the padding under the buttons
	readonly property string command: Fields.stringOf(input, "command")
	readonly property string path: Fields.stringOf(input, "path")
	readonly property string previewDiff: Fields.stringOf(preview, "diff")
	readonly property bool hasPreview: previewDiff !== ""
	readonly property int previewAdded: preview.added ?? 0
	readonly property int previewRemoved: preview.removed ?? 0
	readonly property bool previewTruncated: preview.truncated === true
	readonly property int maxPreviewHeight: 240
	readonly property int maxPreviewLines: 80
	property bool ready: false
	property real openProgress: open ? 1 : 0
	readonly property bool animating: openAnim.running
	readonly property real contentHeight: content.y + content.height

	width: parent.width
	height: openProgress === 1 ? contentHeight : Math.round(contentHeight * openProgress)
	clip: true
	Component.onCompleted: ready = true

	Behavior on openProgress {
		enabled: section.ready
		NumberAnimation { id: openAnim; duration: Motion.standard; easing.type: Motion.ease }
	}
	Rectangle {
		width: parent.width
		height: section.dividerHeight
		color: Theme.border
	}

	Column {
		id: content
		x: section.padding
		y: section.dividerHeight + section.rowSpacing + section.topGap
		width: parent.width - 2 * section.padding
		bottomPadding: section.padding - section.bottomTrim
		spacing: section.rowSpacing

		Rectangle {
			visible: section.command !== ""
			width: parent.width
			height: commandText.height + 2 * section.fieldPadding
			radius: Theme.radiusSmall
			color: Theme.codeBackground

			Text {
				textFormat: Text.PlainText
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
			textFormat: Text.PlainText
			visible: section.path !== ""
			width: parent.width
			text: section.path
			color: Theme.infoText
			font.family: Theme.monoFamily
			font.pixelSize: Theme.codeFontSize
			elide: Text.ElideMiddle
		}
		Column {
			visible: section.hasPreview
			width: parent.width
			spacing: 6

			Row {
				spacing: 8

				Text {
					textFormat: Text.PlainText
					text: qsTr("Preview")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					font.bold: true
				}
				Text {
					textFormat: Text.PlainText
					visible: section.previewAdded > 0
					text: "+" + section.previewAdded
					color: Theme.diffAdded
					font.family: Theme.monoFamily
					font.pixelSize: Theme.smallFontSize
				}
				Text {
					textFormat: Text.PlainText
					visible: section.previewRemoved > 0
					text: "-" + section.previewRemoved
					color: Theme.diffRemoved
					font.family: Theme.monoFamily
					font.pixelSize: Theme.smallFontSize
				}
				Text {
					textFormat: Text.PlainText
					visible: section.previewTruncated
					text: qsTr("(preview cut)")
					color: Theme.warningText
					font.pixelSize: Theme.smallFontSize
				}
			}

			Flickable {
				width: parent.width
				height: Math.min(previewDiffView.height, section.maxPreviewHeight)
				contentWidth: width
				contentHeight: previewDiffView.height
				flickableDirection: Flickable.VerticalFlick
				interactive: contentHeight > height
				boundsBehavior: Flickable.StopAtBounds
				clip: true

				LineView {
					id: previewDiffView
					width: parent.width
					rows: ToolInfo.diffRows(section.previewDiff)
					previewLines: section.maxPreviewLines
					expandable: false
					copyText: section.previewDiff
				}
			}
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
					textTint: Theme.successText
					onClicked: section.answered(true, false)
				}
				PermissionButton {
					iconName: "check-all"
					label: qsTr("Allow for session")
					keyName: "s"
					tint: Theme.accent
					textTint: Theme.accentText
					onClicked: section.answered(true, true)
				}
				PermissionButton {
					iconName: "close"
					label: qsTr("Deny")
					keyName: "d"
					tint: Theme.danger
					textTint: Theme.dangerText
					onClicked: section.answered(false, false)
				}
			}

			Text {
				textFormat: Text.PlainText
				anchors { right: parent.right; verticalCenter: parent.verticalCenter }
				visible: section.othersWaiting > 0
				text: qsTr("+%1 more waiting").arg(section.othersWaiting)
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}
		}
	}
}
