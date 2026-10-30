import QtQuick
import RefGui

// The list over the input box for the `/` command or the `@` file under the cursor. The text
// area keeps the focus and drives it: it moves the selection and asks for the current entry.
// `entries` holds command maps (kind "command") or the model of files (kind "file").
Rectangle {
	id: popup

	readonly property int popupGap: 8
	readonly property int popupPadding: 4
	readonly property int popupRowHeight: 36
	readonly property int popupMaxRows: 8
	readonly property int rowPaddingX: 8
	readonly property int iconSize: 16
	readonly property int rowSpacing: 8

	property string kind: "none"
	property var entries: []
	readonly property int count: view.count
	readonly property bool open: kind !== "none" && count > 0
	property alias currentIndex: view.currentIndex

	signal accepted(int index)

	function move(delta) {
		if (count > 0)
			view.currentIndex = (view.currentIndex + delta + count) % count
	}

	// The command map of an entry, for a command list.
	function commandAt(index) {
		return kind === "command" && index >= 0 && index < entries.length ? entries[index] : null
	}

	readonly property real shownHeight: Math.min(count, popupMaxRows) * popupRowHeight + 2 * popupPadding

	implicitHeight: open ? shownHeight : 0
	height: implicitHeight
	radius: Theme.radius
	color: Theme.surface
	border.color: Theme.border
	opacity: open ? 1 : 0
	visible: opacity > 0
	clip: true

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on opacity {
		NumberAnimation { duration: popup.open ? Motion.standard : Motion.quick; easing.type: Motion.ease }
	}

	transform: Translate {
		y: popup.open ? 0 : popup.popupGap
		Behavior on y { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	// A new result list starts at its best entry.
	onEntriesChanged: view.currentIndex = 0
	onKindChanged: view.currentIndex = 0
	Connections {
		target: popup.kind === "file" ? popup.entries : null
		function onModelReset() { view.currentIndex = 0 }
	}

	ListView {
		id: view
		anchors { fill: parent; margins: popup.popupPadding }
		model: popup.entries
		currentIndex: 0
		clip: true
		highlightFollowsCurrentItem: false
		boundsBehavior: Flickable.StopAtBounds
		delegate: popup.kind === "file" ? fileRow : commandRow

		highlight: Rectangle {
			y: view.currentItem ? view.currentItem.y : 0
			width: view.width
			height: popup.popupRowHeight
			radius: Theme.radiusSmall
			color: Theme.surfaceHover

			Behavior on y { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
	}

	Component {
		id: commandRow

		Item {
			id: row

			required property int index
			required property var modelData

			width: ListView.view.width
			height: popup.popupRowHeight

			Icon {
				id: slashIcon
				x: popup.rowPaddingX
				y: (parent.height - height) / 2
				name: "slash"
				size: popup.iconSize
				color: Theme.textMuted
			}

			Text {
				id: nameLabel
				anchors { left: slashIcon.right; leftMargin: popup.rowSpacing; verticalCenter: parent.verticalCenter }
				text: "/" + row.modelData.slash
				color: Theme.text
				font.pixelSize: Theme.fontSize
			}

			Text {
				id: shortcutLabel
				anchors { right: parent.right; rightMargin: popup.rowPaddingX; verticalCenter: parent.verticalCenter }
				text: row.modelData.shortcut
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}

			Text {
				anchors {
					left: nameLabel.right
					leftMargin: popup.rowSpacing
					right: shortcutLabel.text === "" ? shortcutLabel.right : shortcutLabel.left
					rightMargin: popup.rowSpacing
					verticalCenter: parent.verticalCenter
				}
				horizontalAlignment: Text.AlignRight
				text: row.modelData.title
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				elide: Text.ElideRight
			}

			HoverHandler { onHoveredChanged: if (hovered) view.currentIndex = row.index }
			TapHandler { onTapped: popup.accepted(row.index) }
		}
	}

	Component {
		id: fileRow

		Item {
			id: row

			required property int index
			required property string path
			required property string name
			required property string directory
			required property bool isDirectory

			width: ListView.view.width
			height: popup.popupRowHeight

			Icon {
				id: fileIcon
				x: popup.rowPaddingX
				y: (parent.height - height) / 2
				name: row.isDirectory ? "folder" : "file"
				size: popup.iconSize
				color: Theme.textMuted
			}

			Text {
				anchors {
					left: fileIcon.right
					leftMargin: popup.rowSpacing
					right: parent.right
					rightMargin: popup.rowPaddingX
					verticalCenter: parent.verticalCenter
				}
				textFormat: Text.StyledText
				text: "<font color=\"" + Theme.textMuted + "\">" + popup.htmlEscape(row.directory) + "</font><font color=\"" + Theme.text + "\">"
					+ popup.htmlEscape(row.name) + (row.isDirectory ? "/" : "") + "</font>"
				font.family: Theme.monoFamily
				font.pixelSize: Theme.codeFontSize
				elide: Text.ElideLeft
			}

			HoverHandler { onHoveredChanged: if (hovered) view.currentIndex = row.index }
			TapHandler { onTapped: popup.accepted(row.index) }
		}
	}

	function htmlEscape(text) {
		return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
	}
}
