import QtQuick
import QtQuick.Controls
import RefGui

// A small pop-up menu. `entries` is a list of {text, danger}; `triggered(index)` tells the choice.
Popup {
	id: menu

	property var entries: []

	signal triggered(int index)

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

				width: Math.max(150, label.implicitWidth + 2 * 14)
				height: 32

				Rectangle {
					anchors.fill: parent
					radius: Theme.radiusSmall + 2
					color: hover.hovered ? Theme.surfaceHover : "transparent"

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
				Text {
					id: label
					anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
					text: entry.modelData.text
					color: entry.modelData.danger ? Theme.danger : Theme.text
					font.pixelSize: Theme.fontSize - 1
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
