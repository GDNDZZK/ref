pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// The list over the input box for `/`, `@`, and `!` completions under the cursor. The text
// area keeps the focus and drives it: it moves the selection and asks for the current entry.
Rectangle {
	id: popup

	readonly property int popupGap: 8
	readonly property int popupPadding: 4
	readonly property int popupRowHeight: 36
	readonly property int popupMaxRows: 8
	readonly property int rowPaddingX: 8
	readonly property int iconSize: 16
	readonly property int rowSpacing: 8

	property string kind: CompletionKind.none
	property var entries: []
	property var fileModel: null
	property var itemModel: null
	readonly property int count: view.count
	readonly property bool open: kind !== CompletionKind.none && count > 0
	property alias currentIndex: view.currentIndex
	// True once the user navigated or pointed at an entry in the current list.
	property bool explicitlySelected: false

	property bool animateHighlight: false

	signal accepted(int index)

	function resetSelection() {
		animateHighlight = false
		explicitlySelected = false
		hoverGuard.reset()
		view.currentIndex = 0
		view.positionViewAtBeginning()
	}

	function move(delta) {
		if (count > 0) {
			animateHighlight = true
			explicitlySelected = true
			view.currentIndex = (view.currentIndex + delta + count) % count
		}
	}

	// The command map of an entry, for a command list.
	function commandAt(index) {
		const isCommandIndex = kind === CompletionKind.command && index >= 0 && index < entries.length
		if (!isCommandIndex)
			return null
		return entries[index]
	}

	function syncView() {
		if (kind === CompletionKind.none) {
			view.model = null
			resetSelection()
			return
		}
		const wantedDelegate = kind === CompletionKind.file ? fileRow
			: (kind === CompletionKind.command ? commandRow : entryRow)
		const wantedModel = kind === CompletionKind.file ? fileModel
			: (kind === CompletionKind.command ? entries : itemModel)
		if (view.delegate !== wantedDelegate) {
			view.model = null
			view.delegate = wantedDelegate
		}
		if (view.model !== wantedModel)
			view.model = wantedModel
		resetSelection()
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

	// A new result list starts at its best entry; delegate and model switch atomically.
	onEntriesChanged: if (kind === CompletionKind.command) syncView()
	onFileModelChanged: if (kind === CompletionKind.file) syncView()
	onItemModelChanged: if (kind !== CompletionKind.none && kind !== CompletionKind.command && kind !== CompletionKind.file) syncView()
	onKindChanged: syncView()
	Component.onCompleted: syncView()
	onOpenChanged: if (!open) { hoverGuard.reset(); explicitlySelected = false }
	PointerMoveGuard { id: hoverGuard }
	Connections {
		target: popup.kind === CompletionKind.file ? popup.fileModel
			: (popup.kind !== CompletionKind.none && popup.kind !== CompletionKind.command ? popup.itemModel : null)
		function onModelReset() { popup.resetSelection() }
	}
	ListView {
		id: view
		anchors { fill: parent; margins: popup.popupPadding }
		currentIndex: 0
		clip: true
		highlightFollowsCurrentItem: false
		boundsBehavior: Flickable.StopAtBounds

		highlight: Rectangle {
			y: view.currentItem ? view.currentItem.y : 0
			width: view.width
			height: popup.popupRowHeight
			radius: Theme.radiusSmall
			color: Theme.surfaceHover

			Behavior on y {
				enabled: popup.animateHighlight
				NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
			}
		}

		onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)

		HoverHandler {
			onPointChanged: {
				if (!hovered || !hoverGuard.movedTo(point.scenePosition))
					return
				const index = view.indexAt(point.position.x + view.contentX, point.position.y + view.contentY)
				if (index >= 0) {
					popup.animateHighlight = true
					popup.explicitlySelected = true
					view.currentIndex = index
				}
			}
		}
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
				textFormat: Text.PlainText
				anchors { left: slashIcon.right; leftMargin: popup.rowSpacing; verticalCenter: parent.verticalCenter }
				text: "/" + row.modelData.slash
				color: Theme.text
				font.pixelSize: Theme.fontSize
			}

			Text {
				id: shortcutLabel
				textFormat: Text.PlainText
				anchors { right: parent.right; rightMargin: popup.rowPaddingX; verticalCenter: parent.verticalCenter }
				text: row.modelData.shortcut
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}

			Text {
				textFormat: Text.PlainText
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

			HoverHandler { cursorShape: Qt.PointingHandCursor }
			TapHandler {
				onTapped: {
					popup.explicitlySelected = true
					popup.accepted(row.index)
				}
			}
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

			// The directory part is muted and is what gets cut when the path is too long.
			Row {
				id: pathRow
				anchors { left: fileIcon.right; leftMargin: popup.rowSpacing; verticalCenter: parent.verticalCenter }
				width: parent.width - x - popup.rowPaddingX

				Text {
					textFormat: Text.PlainText
					width: Math.min(implicitWidth, pathRow.width - fileName.implicitWidth)
					text: row.directory
					color: Theme.textMuted
					font.family: Theme.monoFamily
					font.pixelSize: Theme.codeFontSize
					elide: Text.ElideLeft
				}

				Text {
					id: fileName
					textFormat: Text.PlainText
					text: row.name + (row.isDirectory ? (row.path.endsWith("\\") ? "\\" : "/") : "")
					color: Theme.text
					font.family: Theme.monoFamily
					font.pixelSize: Theme.codeFontSize
				}
			}

			HoverHandler { cursorShape: Qt.PointingHandCursor }
			TapHandler {
				onTapped: {
					popup.explicitlySelected = true
					popup.accepted(row.index)
				}
			}
		}
	}

	Component {
		id: entryRow

		Item {
			id: row

			required property int index
			required property string value
			required property string label
			required property string prefixLabel
			required property string detail
			required property string icon
			required property bool isDirectory
			required property bool mono

			width: ListView.view.width
			height: popup.popupRowHeight

			Icon {
				id: entryIcon
				x: popup.rowPaddingX
				y: (parent.height - height) / 2
				name: row.icon !== "" ? row.icon : (row.isDirectory ? "folder" : "slash")
				size: popup.iconSize
				color: Theme.textMuted
			}

			Row {
				id: labelRow
				anchors { left: entryIcon.right; leftMargin: popup.rowSpacing; verticalCenter: parent.verticalCenter }
				readonly property real maxLabelRowWidth: Math.max(80, row.width - x - popup.rowPaddingX - (detailLabel.visible ? Math.min(detailLabel.implicitWidth, Math.floor(row.width * 0.42)) + popup.rowSpacing : 0))
				width: Math.min(prefixText.implicitWidth + mainLabel.implicitWidth, maxLabelRowWidth)

				Text {
					id: prefixText
					textFormat: Text.PlainText
					width: Math.min(implicitWidth, Math.max(0, labelRow.maxLabelRowWidth - mainLabel.implicitWidth))
					text: row.prefixLabel
					color: Theme.textMuted
					font.family: row.mono ? Theme.monoFamily : undefined
					font.pixelSize: row.mono ? Theme.codeFontSize : Theme.fontSize
					elide: Text.ElideLeft
					visible: row.prefixLabel !== ""
				}

				Text {
					id: mainLabel
					textFormat: Text.PlainText
					width: Math.min(implicitWidth, labelRow.maxLabelRowWidth)
					text: row.label + (row.isDirectory ? (row.value.endsWith("\\") ? "\\" : "/") : "")
					color: Theme.text
					font.family: row.mono ? Theme.monoFamily : undefined
					font.pixelSize: row.mono ? Theme.codeFontSize : Theme.fontSize
					elide: Text.ElideRight
				}
			}

			Text {
				id: detailLabel
				textFormat: Text.PlainText
				anchors {
					left: labelRow.right
					leftMargin: popup.rowSpacing
					right: parent.right
					rightMargin: popup.rowPaddingX
					verticalCenter: parent.verticalCenter
				}
				horizontalAlignment: Text.AlignRight
				text: row.detail
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				elide: Text.ElideRight
				visible: row.detail !== ""
			}

			HoverHandler { cursorShape: Qt.PointingHandCursor }
			TapHandler {
				onTapped: {
					popup.explicitlySelected = true
					popup.accepted(row.index)
				}
			}
		}
	}
}
