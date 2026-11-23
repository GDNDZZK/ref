pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// A small pop-up menu. `entries` is a list of {text, danger}; `triggered(index)` tells the choice.
Popup {
	id: menu

	property var entries: []
	readonly property int textInset: 14
	readonly property int menuPadding: 4
	readonly property real uiScale: UiScale.factor

	signal triggered(int index)

	// Opens the menu with its corner at `point`, in the coordinates of the parent item.
	function openAt(point: point) {
		x = point.x
		y = point.y
		open()
	}

	width: menuInner.width * uiScale
	height: menuInner.height * uiScale
	padding: 0
	modal: true
	focus: true
	Overlay.modal: Rectangle { color: "transparent" }
	closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
	background: null

	enter: Transition {
		NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.quick; easing.type: Motion.ease }
		NumberAnimation { property: "scale"; from: Motion.popScale; to: 1; duration: Motion.quick; easing.type: Motion.ease }
	}
	exit: Transition {
		NumberAnimation { property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
	}

	contentItem: Item {
		// Swallow clicks inside the popup padding so they cannot bleed through to rows underneath.
		MouseArea {
			anchors.fill: parent
		}

		Item {
			id: menuInner
			objectName: "contextMenuInner"
			width: menuList.implicitWidth + 2 * menu.menuPadding
			height: menuList.implicitHeight + 2 * menu.menuPadding
			transformOrigin: Item.TopLeft
			scale: menu.uiScale

			Rectangle {
				anchors.fill: parent
				radius: Theme.radius
				color: Theme.surface
				border.color: Theme.border
			}

			Column {
				id: menuList
				x: menu.menuPadding
				y: menu.menuPadding
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
							color: entryMouse.containsMouse ? Theme.surfaceHover : "transparent"

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
						MouseArea {
							id: entryMouse
							anchors.fill: parent
							hoverEnabled: true
							cursorShape: Qt.PointingHandCursor
							onClicked: {
								menu.close()
								menu.triggered(entry.index)
							}
						}
					}
				}
			}
		}
	}
}
