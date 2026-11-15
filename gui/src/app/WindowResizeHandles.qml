pragma ComponentBehavior: Bound
import QtQuick

// Invisible edge and corner hit targets for resizing a frameless window.
Item {
	id: root

	property bool active: false
	readonly property int thickness: 6

	signal resizeRequested(int edges)

	visible: active
	enabled: active

	component EdgeHandle: MouseArea {
		required property int edges

		acceptedButtons: Qt.LeftButton
		onPressed: mouse => {
			mouse.accepted = true
			root.resizeRequested(edges)
		}
	}

	EdgeHandle {
		edges: Qt.TopEdge
		x: root.thickness
		y: 0
		width: Math.max(0, parent.width - 2 * root.thickness)
		height: root.thickness
		cursorShape: Qt.SizeVerCursor
	}
	EdgeHandle {
		edges: Qt.BottomEdge
		x: root.thickness
		y: parent.height - root.thickness
		width: Math.max(0, parent.width - 2 * root.thickness)
		height: root.thickness
		cursorShape: Qt.SizeVerCursor
	}
	EdgeHandle {
		edges: Qt.LeftEdge
		x: 0
		y: root.thickness
		width: root.thickness
		height: Math.max(0, parent.height - 2 * root.thickness)
		cursorShape: Qt.SizeHorCursor
	}
	EdgeHandle {
		edges: Qt.RightEdge
		x: parent.width - root.thickness
		y: root.thickness
		width: root.thickness
		height: Math.max(0, parent.height - 2 * root.thickness)
		cursorShape: Qt.SizeHorCursor
	}
	EdgeHandle {
		edges: Qt.TopEdge | Qt.LeftEdge
		x: 0
		y: 0
		width: root.thickness
		height: root.thickness
		cursorShape: Qt.SizeFDiagCursor
	}
	EdgeHandle {
		edges: Qt.TopEdge | Qt.RightEdge
		x: parent.width - root.thickness
		y: 0
		width: root.thickness
		height: root.thickness
		cursorShape: Qt.SizeBDiagCursor
	}
	EdgeHandle {
		edges: Qt.BottomEdge | Qt.LeftEdge
		x: 0
		y: parent.height - root.thickness
		width: root.thickness
		height: root.thickness
		cursorShape: Qt.SizeBDiagCursor
	}
	EdgeHandle {
		edges: Qt.BottomEdge | Qt.RightEdge
		x: parent.width - root.thickness
		y: parent.height - root.thickness
		width: root.thickness
		height: root.thickness
		cursorShape: Qt.SizeFDiagCursor
	}
}
