import QtQuick
import RefGui

// One picture at full size over the pane, on a dark scrim. Esc, a click outside the picture
// or the close button put it away.
Item {
	id: lightbox

	property string sessionId
	property var image: null // {id, mime, size, name}

	readonly property int margin: 48
	readonly property real scrimOpacity: 0.72
	readonly property real closedScale: 0.96
	readonly property int captionHeight: 24
	readonly property bool shown: image !== null

	signal closed()

	function open(session, picture) {
		sessionId = session
		image = picture
		forceActiveFocus()
	}

	function close() {
		if (image === null)
			return
		image = null
		closed()
	}

	anchors.fill: parent
	visible: opacity > 0
	opacity: shown ? 1 : 0
	z: 100 // above everything of the pane

	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Keys.onEscapePressed: close()

	Rectangle {
		anchors.fill: parent
		color: Qt.rgba(0, 0, 0, lightbox.scrimOpacity)

		TapHandler { onTapped: lightbox.close() }
	}

	Item {
		id: stage
		anchors { fill: parent; margins: lightbox.margin; bottomMargin: lightbox.margin + lightbox.captionHeight }
		scale: lightbox.shown ? 1 : lightbox.closedScale

		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		// Not enlarged past its own size: a small picture would only turn blurry.
		Image {
			anchors.centerIn: parent
			width: Math.min(parent.width, implicitWidth)
			height: Math.min(parent.height, implicitHeight)
			asynchronous: true
			fillMode: Image.PreserveAspectFit
			source: lightbox.image && lightbox.image.id ? "image://chatimage/" + lightbox.sessionId + "/" + lightbox.image.id : ""
			cache: false
		}
	}

	Text {
		textFormat: Text.PlainText
		anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: lightbox.margin / 2 }
		text: lightbox.image ? [lightbox.image.name ?? "", ToolInfo.sizeText(lightbox.image.size ?? 0)].filter(part => part !== "").join(" \u00b7 ") : ""
		color: Qt.rgba(1, 1, 1, 0.8)
		font.pixelSize: Theme.smallFontSize
	}

	CircleButton {
		anchors { top: parent.top; right: parent.right; margins: 12 }
		Accessible.name: qsTr("Close")
		onClicked: lightbox.close()

		Icon {
			anchors.centerIn: parent
			size: 20
			name: "close"
			color: Theme.textMuted
		}
	}
}
