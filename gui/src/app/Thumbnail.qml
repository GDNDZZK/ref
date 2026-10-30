import QtQuick
import RefGui

// A picture of the conversation at thumbnail size: a placeholder until the backend has sent
// the bytes, an error mark when it cannot. Hover dims it and offers the enlargement.
Item {
	id: thumbnail

	required property string sessionId
	required property var image // {id, mime, size, name}
	property int maxWidth: 160
	property int maxHeight: 96

	signal clicked()

	readonly property bool ready: picture.status === Image.Ready
	readonly property bool failed: picture.status === Image.Error
	readonly property real hoverDim: 0.12
	readonly property int cornerIcon: 20

	width: ready ? picture.paintedWidth : maxWidth
	height: ready ? picture.paintedHeight : maxHeight
	clip: true

	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: Theme.surface
		border.color: Theme.border

		Icon {
			anchors.centerIn: parent
			size: 20
			name: thumbnail.failed ? "error" : "image"
			color: thumbnail.failed ? Theme.danger : Theme.textMuted
			visible: !thumbnail.ready
		}
	}

	Image {
		id: picture
		anchors.centerIn: parent
		width: thumbnail.maxWidth
		height: thumbnail.maxHeight
		asynchronous: true
		fillMode: Image.PreserveAspectFit
		sourceSize: Qt.size(thumbnail.maxWidth, thumbnail.maxHeight) // logical: Image scales it by the device pixel ratio
		source: thumbnail.image.id ? "image://chatimage/" + thumbnail.sessionId + "/" + thumbnail.image.id : ""
	}

	// Rounds the corners of the picture and draws its border over it.
	Rectangle {
		anchors.fill: parent
		radius: Theme.radiusSmall
		color: Qt.rgba(0, 0, 0, hover.hovered && thumbnail.ready ? thumbnail.hoverDim : 0)
		border.color: Theme.border

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Icon {
			anchors { right: parent.right; bottom: parent.bottom; margins: 4 }
			size: thumbnail.cornerIcon
			name: "maximize"
			color: "white"
			opacity: hover.hovered && thumbnail.ready ? 1 : 0

			Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}
	}

	HoverHandler { id: hover; cursorShape: thumbnail.ready ? Qt.PointingHandCursor : Qt.ArrowCursor }
	TapHandler {
		enabled: thumbnail.ready
		onTapped: thumbnail.clicked()
	}
	Accessible.role: Accessible.Button
	Accessible.name: thumbnail.image.name ?? qsTr("Image")
}
