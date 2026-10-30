import QtQuick
import QtQuick.Controls
import RefGui

// A search field above a list whose current row has a highlight that slides. Up / Down move
// the highlight, Enter activates the row. The rows are PickRow items from `delegate`; the
// owner filters `model` by `text`.
Item {
	id: pick

	property alias model: view.model
	property alias delegate: view.delegate
	property alias text: input.text
	property string placeholderText: qsTr("Search")
	property string sectionProperty: ""
	// The label of a section from the value of its property.
	property var sectionLabel: section => section
	property int visibleHeight: 7 * 44 // the list is this tall at most
	property string emptyText: qsTr("Nothing found")
	property bool searchable: true
	readonly property alias count: view.count

	function reset() {
		view.currentIndex = view.count > 0 ? 0 : -1
	}

	function activateCurrent() {
		if (view.currentItem && view.currentItem.enabled)
			view.currentItem.activated()
	}

	// Moves the highlight by one row, wrapping around and skipping rows that cannot be chosen.
	function move(step) {
		let index = view.currentIndex
		for (let tries = 0; tries < view.count; ++tries) {
			index = (index + step + view.count) % view.count
			const item = view.itemAtIndex(index)
			if (item === null || item.enabled) {
				view.currentIndex = index
				return
			}
		}
	}

	implicitHeight: (searchable ? Theme.paletteSearchHeight + 1 : 0) + Math.max(Math.min(view.contentHeight, visibleHeight), view.count === 0 ? 64 : 0)

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	onCountChanged: reset()
	Component.onCompleted: if (searchable) input.forceActiveFocus()

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
			Accessible.name: pick.placeholderText
			Keys.onUpPressed: pick.move(-1)
			Keys.onDownPressed: pick.move(1)
			Keys.onReturnPressed: event => pick.activateCurrent()
			Keys.onEnterPressed: event => pick.activateCurrent()

			Text {
				anchors.fill: parent
				verticalAlignment: Text.AlignVCenter
				text: pick.placeholderText
				color: Theme.textMuted
				font.pixelSize: Theme.fontSize
				visible: input.text === "" && input.preeditText === ""
			}
		}

		CircleButton {
			id: clear
			anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
			implicitWidth: 24
			implicitHeight: 24
			opacity: input.text !== "" ? 1 : 0
			visible: opacity > 0
			Accessible.name: qsTr("Clear search")
			onClicked: {
				input.text = ""
				input.forceActiveFocus()
			}

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			Icon {
				anchors.centerIn: parent
				name: "close"
				size: 16
				color: Theme.textMuted
			}
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

			Text {
				x: 18
				anchors.verticalCenter: parent.verticalCenter
				text: pick.sectionLabel(parent.section)
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
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

			Behavior on y { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		ScrollBar.vertical: ThemedScrollBar {}
	}

	Text {
		anchors { horizontalCenter: parent.horizontalCenter; top: divider.bottom; topMargin: 22 }
		text: pick.emptyText
		color: Theme.textMuted
		font.pixelSize: Theme.fontSize - 1
		visible: view.count === 0
	}
}
