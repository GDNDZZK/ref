pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Shapes
import RefGui

// The images attached to the prompt: square thumbnails in a row that scrolls sideways when
// full. Each has a remove button that shows on hover.
ListView {
	id: strip

	readonly property int thumbSize: 56
	readonly property int thumbGap: 8
	readonly property int attachmentRemoveSize: 18
	readonly property int crossSize: 8
	readonly property real crossStroke: 1.5
	readonly property real removeBackgroundOpacity: 0.85
	readonly property real enterScale: 0.8
	readonly property int removeInset: 4
	readonly property int tipDelay: 600

	signal removeRequested(int row)

	implicitHeight: thumbSize
	orientation: ListView.Horizontal
	spacing: thumbGap
	interactive: contentWidth > width
	clip: true
	boundsBehavior: Flickable.StopAtBounds

	delegate: Item {
		id: thumb

		required property int index
		required property string name
		required property string source

		width: strip.thumbSize
		height: strip.thumbSize

		Rectangle {
			id: frame
			anchors.fill: parent
			radius: Theme.radius
			color: Theme.surfaceHover
			clip: true

			Image {
				anchors.fill: parent
				source: thumb.source
				fillMode: Image.PreserveAspectCrop
				asynchronous: true
				sourceSize: Qt.size(strip.thumbSize * Screen.devicePixelRatio, strip.thumbSize * Screen.devicePixelRatio)
			}
		}

		HoverHandler {
			id: hover
			cursorShape: Qt.ArrowCursor
		}

		Rectangle {
			id: remove
			readonly property bool shown: hover.hovered || removeTap.pressed
			anchors { top: parent.top; right: parent.right; margins: strip.removeInset }
			width: strip.attachmentRemoveSize
			height: strip.attachmentRemoveSize
			radius: width / 2
			color: Theme.background
			opacity: shown ? strip.removeBackgroundOpacity : 0
			visible: opacity > 0

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			Shape {
				anchors.centerIn: parent
				width: strip.crossSize
				height: strip.crossSize
				preferredRendererType: Shape.CurveRenderer

				ShapePath {
					strokeColor: Theme.text
					strokeWidth: strip.crossStroke
					fillColor: "transparent"
					capStyle: ShapePath.RoundCap

					startX: 0
					startY: 0
					PathLine { x: strip.crossSize; y: strip.crossSize }
					PathMove { x: strip.crossSize; y: 0 }
					PathLine { x: 0; y: strip.crossSize }
				}
			}

			TapHandler {
				id: removeTap
				onTapped: strip.removeRequested(thumb.index)
			}
			Accessible.role: Accessible.Button
			Accessible.name: qsTr("Remove %1").arg(thumb.name)
			Accessible.onPressAction: strip.removeRequested(thumb.index)
		}

		PlainToolTip {
			visible: hover.hovered
			text: thumb.name
			delay: strip.tipDelay
		}
	}

	add: Transition {
		ParallelAnimation {
			NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
			NumberAnimation { property: "scale"; from: strip.enterScale; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		}
	}
	remove: Transition {
		ParallelAnimation {
			NumberAnimation { property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
			NumberAnimation { property: "scale"; to: strip.enterScale; duration: Motion.quick; easing.type: Motion.ease }
		}
	}
	displaced: Transition {
		NumberAnimation { properties: "x"; duration: Motion.standard; easing.type: Motion.ease }
	}
}
