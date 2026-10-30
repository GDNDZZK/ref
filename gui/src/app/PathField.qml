import QtQuick
import RefGui

// A path field for a directory on the backend's machine, with the sub-directories that
// continue what was typed listed below. Tab or a click takes a name; Up / Down choose.
Item {
	id: path

	property alias text: field.text
	property alias placeholderText: field.placeholderText
	property alias errorText: field.errorText
	property var names: []
	property string listedPath: ""
	property int request: 0
	readonly property int visibleRows: 6
	readonly property int rowHeight: 32
	readonly property bool completing: names.length > 0

	signal accepted()

	function select() {
		field.selectAll()
	}

	// The part up to the last slash is the directory to list; the rest is what its names must start with.
	function refresh() {
		const slash = text.lastIndexOf("/")
		const directory = slash < 0 ? "." : text.slice(0, slash + 1)
		const prefix = text.slice(slash + 1).toLowerCase()
		const mine = ++request
		Workspace.listDirs(directory, (ok, listing) => {
			if (mine !== request)
				return
			listedPath = ok ? listing.path : ""
			names = ok ? listing.dirs.filter(name => name.toLowerCase().startsWith(prefix)) : []
			view.currentIndex = names.length > 0 ? 0 : -1
		})
	}

	function accept(name) {
		const base = listedPath === "/" ? "" : listedPath
		text = base + "/" + name + "/"
		field.inputItem.cursorPosition = text.length
		refresh()
	}

	implicitHeight: field.implicitHeight + (completing ? 8 + Math.min(names.length, visibleRows) * rowHeight : 0)

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Timer {
		id: debounce
		interval: 150
		onTriggered: path.refresh()
	}
	Component.onCompleted: refresh()
	Component.onDestruction: request = -1

	DialogField {
		id: field
		width: parent.width
		onAccepted: path.accepted()
		onEdited: debounce.restart()

		onTabPressed: {
			if (view.currentIndex >= 0)
				path.accept(path.names[view.currentIndex])
		}
		onDownPressed: if (path.completing) view.currentIndex = Math.min(view.currentIndex + 1, path.names.length - 1)
		onUpPressed: if (path.completing) view.currentIndex = Math.max(view.currentIndex - 1, 0)
	}

	ListView {
		id: view
		y: field.height + 8
		width: parent.width
		height: Math.min(path.names.length, path.visibleRows) * path.rowHeight
		clip: true
		model: path.names
		boundsBehavior: Flickable.StopAtBounds
		highlightFollowsCurrentItem: false
		visible: path.completing

		highlight: Rectangle {
			y: view.currentItem ? view.currentItem.y : 0
			width: view.width
			height: path.rowHeight
			radius: 8
			color: Theme.surfaceHover
			visible: view.currentItem !== null

			Behavior on y { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		delegate: Item {
			id: row
			required property string modelData
			required property int index
			width: view.width
			height: path.rowHeight

			Icon {
				x: 12
				anchors.verticalCenter: parent.verticalCenter
				name: "folder"
				size: 16
				color: Theme.textMuted
			}
			Text {
				anchors { left: parent.left; leftMargin: 38; right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: row.modelData
				color: Theme.text
				font.pixelSize: Theme.fontSize - 1
				elide: Text.ElideRight
			}
			HoverHandler {
				cursorShape: Qt.PointingHandCursor
				onHoveredChanged: if (hovered) view.currentIndex = row.index
			}
			TapHandler { onTapped: path.accept(row.modelData) }
		}
	}
}
