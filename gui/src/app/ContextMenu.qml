pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// A small pop-up menu. `entries` is a list of {text, danger}; `triggered(index)` tells the choice.
Popup {
	id: menu

	property var entries: []
	readonly property int textInset: 14

	signal triggered(int index)

	// Opens the menu with its corner at `point`, in the coordinates of the parent item.
	function openAt(point: point) {
		x = point.x
		y = point.y
		open()
	}

	padding: 4
	closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

	enter: Transition {
		NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.quick; easing.type: Motion.ease }
		NumberAnimation { property: "scale"; from: Motion.popScale; to: 1; duration: Motion.quick; easing.type: Motion.ease }
	}
	exit: Transition {
		NumberAnimation { property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
	}

	background: Rectangle {
		radius: Theme.radius
		color: Theme.surface
		border.color: Theme.border
	}

	contentItem: Column {
		spacing: 2

		Repeater {
			model: menu.entries

			Item {
				id: entry

				required property var modelData
				required property int index

				width: Math.max(150, label.implicitWidth + 2 * menu.textInset)
				height: 32

				Rectangle {
					anchors.fill: parent
					radius: Theme.radiusSmall + 2
					color: hover.hovered ? Theme.surfaceHover : "transparent"

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
				Text {
					textFormat: Text.PlainText
					id: label
					anchors { left: parent.left; leftMargin: menu.textInset; verticalCenter: parent.verticalCenter }
					text: entry.modelData.text
					color: entry.modelData.danger ? Theme.dangerText : Theme.text
					font.pixelSize: Theme.secondaryFontSize
				}
				HoverHandler {
					id: hover
					cursorShape: Qt.PointingHandCursor
				}
				TapHandler {
					onTapped: {
						menu.close()
						menu.triggered(entry.index)
					}
				}
			}
		}
	}
}
