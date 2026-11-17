pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// A search field above a list whose current row has a highlight that slides. Up / Down move
// the highlight, Enter activates the row. The rows are PickRow items from `delegate`; the
// owner filters `model` by `text`.
FocusScope {
	id: pick

	property alias model: view.model
	property alias delegate: view.delegate
	property alias text: input.text
	property string placeholderText: qsTr("Search")
	property string sectionProperty: ""
	// The label of a section from the value of its property.
	property var sectionLabel: section => section
	property int visibleHeight: 7 * Theme.pickRowHeight // the list is this tall at most
	property string emptyText: qsTr("Nothing found")
	property bool searchable: true
	property int initialIndex: -1
	// Optional `index => bool` so off-screen disabled rows are skipped without instantiating delegates.
	property var rowEnabled: null
	readonly property alias count: view.count
	readonly property alias currentIndex: view.currentIndex
	readonly property alias currentItem: view.currentItem
	property bool ready: false
	property bool animateHighlight: false
	property bool pendingInitialCenter: false

	function isRowEnabled(index) {
		if (index < 0 || index >= view.count)
			return false
		if (rowEnabled !== null)
			return Boolean(rowEnabled(index))
		const item = view.itemAtIndex(index)
		return item === null || item.enabled
	}

	function firstEnabledIndex() {
		for (let i = 0; i < view.count; ++i) {
			if (isRowEnabled(i))
				return i
		}
		return -1
	}

	function jumpTo(index, mode) {
		animateHighlight = false
		view.currentIndex = index
		if (index >= 0 && index < view.count) {
			view.positionViewAtIndex(index, mode)
			view.forceLayout()
		}
	}

	function reset() {
		hoverGuard.reset()
		const useInitial = input.text === "" && initialIndex >= 0 && initialIndex < view.count && isRowEnabled(initialIndex)
		const target = useInitial ? initialIndex : firstEnabledIndex()
		pendingInitialCenter = useInitial && target > 0
		jumpTo(target, useInitial ? ListView.Center : ListView.Beginning)
	}

	function activateCurrent() {
		if (!isRowEnabled(view.currentIndex))
			return
		if (view.currentItem === null) {
			animateHighlight = false
			view.positionViewAtIndex(view.currentIndex, ListView.Contain)
			view.forceLayout()
		}
		if (view.currentItem && view.currentItem.enabled)
			view.currentItem.activated()
	}

	// Moves the highlight by one row, wrapping around and skipping rows that cannot be chosen.
	function move(step) {
		if (view.count === 0)
			return
		pendingInitialCenter = false
		let index = view.currentIndex
		if (index < 0)
			index = step > 0 ? -1 : 0
		for (let tries = 0; tries < view.count; ++tries) {
			index = (index + step + view.count) % view.count
			if (!isRowEnabled(index))
				continue
			if (rowEnabled === null) {
				view.positionViewAtIndex(index, ListView.Contain)
				view.forceLayout()
				const item = view.itemAtIndex(index)
				if (item !== null && !item.enabled)
					continue
			}
			animateHighlight = true
			view.currentIndex = index
			view.positionViewAtIndex(index, ListView.Contain)
			return
		}
	}

	focus: searchable
	implicitHeight: (searchable ? Theme.paletteSearchHeight + 1 : 0) + Math.max(Math.min(view.contentHeight, visibleHeight), view.count === 0 ? Theme.emptyStateHeight : 0)

	Behavior on implicitHeight {
		enabled: pick.ready
		NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
	}

	Keys.onUpPressed: move(-1)
	Keys.onDownPressed: move(1)
	Keys.onReturnPressed: activateCurrent()
	Keys.onEnterPressed: activateCurrent()

	onCountChanged: reset()
	onInitialIndexChanged: if (input.text === "") reset()
	Component.onCompleted: {
		if (searchable)
			input.forceActiveFocus()
		else if (focus)
			forceActiveFocus()
		Qt.callLater(() => { ready = true })
	}

	PointerMoveGuard { id: hoverGuard }
	Item {
		id: searchRow
		visible: pick.searchable
		width: parent.width
		height: pick.searchable ? Theme.paletteSearchHeight : 0

		Icon {
			x: 18
			anchors.verticalCenter: parent.verticalCenter
			name: "search"
			size: 16
			color: Theme.textMuted
		}

		TextInput {
			id: input
			anchors { left: parent.left; leftMargin: 46; right: clear.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.fontSize
			clip: true
			focus: pick.searchable
			Accessible.name: pick.placeholderText
			Keys.onUpPressed: pick.move(-1)
			Keys.onDownPressed: pick.move(1)
			Keys.onReturnPressed: pick.activateCurrent()
			Keys.onEnterPressed: pick.activateCurrent()
			PlaceholderText {
				anchors.fill: parent
				textInput: input
				text: pick.placeholderText
			}
		}

		IconButton {
			id: clear
			anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
			buttonSize: 24
			iconName: "close"
			iconSize: 16
			opacity: input.text !== "" ? 1 : 0
			visible: opacity > 0
			Accessible.name: qsTr("Clear search")
			onClicked: {
				input.text = ""
				input.forceActiveFocus()
			}

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}
	}

	Rectangle {
		id: divider
		visible: pick.searchable
		y: searchRow.height
		width: parent.width
		height: 1
		color: Theme.border
	}

	ListView {
		id: view
		y: divider.y + (pick.searchable ? 1 : 0)
		width: parent.width
		height: parent.height - y
		clip: true
		currentIndex: 0
		boundsBehavior: Flickable.StopAtBounds
		highlightFollowsCurrentItem: false
		keyNavigationEnabled: false
		section.property: pick.sectionProperty
		section.delegate: Item {
			required property string section
			width: view.width
			height: Theme.sidebarSectionHeight

			SectionLabel {
				leftPadding: Theme.listLabelInset
				text: pick.sectionLabel(parent.section)
			}
		}

		highlight: Rectangle {
			x: 8
			y: view.currentItem ? view.currentItem.y : 0
			width: view.width - 16
			height: view.currentItem ? view.currentItem.height : 0
			radius: 8
			color: Theme.surfaceHover
			visible: view.currentItem !== null

			Behavior on y {
				enabled: pick.animateHighlight
				NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
			}
		}

		onHeightChanged: {
			if (!pick.pendingInitialCenter || height <= 0 || view.currentIndex < 0)
				return
			pick.jumpTo(view.currentIndex, ListView.Center)
			const targetListHeight = Math.min(contentHeight, pick.visibleHeight)
			if (height >= targetListHeight - 1)
				pick.pendingInitialCenter = false
		}

		HoverHandler {
			onPointChanged: {
				if (!hovered || !hoverGuard.movedTo(point.scenePosition))
					return
				const index = view.indexAt(point.position.x + view.contentX, point.position.y + view.contentY)
				if (index >= 0 && pick.isRowEnabled(index)) {
					pick.animateHighlight = true
					view.currentIndex = index
				}
			}
		}

		ScrollBar.vertical: ThemedScrollBar {}
	}

	Text {
		textFormat: Text.PlainText
		anchors { horizontalCenter: parent.horizontalCenter; top: divider.bottom; topMargin: 22 }
		text: pick.emptyText
		color: Theme.textMuted
		font.pixelSize: Theme.secondaryFontSize
		visible: view.count === 0
	}
}
