pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Pictures at full size over the pane, on a dark scrim. Supports prev/next navigation,
// mouse-wheel and toolbar zoom, drag-to-pan, and double-click zoom toggle.
Item {
	id: lightbox

	property string sessionId
	property var image: null // {id, mime, size, name}
	property var images: []
	property int currentIndex: -1
	property real zoom: 1.0
	property real panX: 0
	property real panY: 0

	readonly property int margin: 48
	readonly property real scrimOpacity: 0.72
	readonly property real closedScale: 0.96
	readonly property int captionHeight: 24
	readonly property int toolbarHeight: 34
	readonly property int toolbarBottomGap: 10
	readonly property int navMargin: 16
	readonly property real minZoom: 0.25
	readonly property real maxZoom: 5.0
	readonly property real zoomStep: 1.25
	readonly property real wheelZoomStep: 1.15
	readonly property real doubleClickZoom: 2.0
	readonly property bool shown: image !== null
	readonly property bool hasMultiple: images.length > 1
	readonly property string counterText: hasMultiple && currentIndex >= 0 ? (currentIndex + 1) + " / " + images.length : ""
	readonly property string captionText: {
		if (!image)
			return ""
		const parts = [counterText, image.name ?? "", ToolInfo.sizeText(image.size ?? 0)].filter(part => part !== "")
		return parts.join(" \u00b7 ")
	}

	signal closed()

	function resetView() {
		zoom = 1.0
		panX = 0
		panY = 0
	}

	function clampZoom(value) {
		return Math.max(minZoom, Math.min(maxZoom, value))
	}

	function setZoom(value) {
		const next = clampZoom(value)
		zoom = next
		if (next <= 1.0) {
			panX = 0
			panY = 0
		}
	}

	function zoomBy(factor) {
		setZoom(zoom * factor)
	}

	function open(session, picture, list) {
		sessionId = session
		const items = (list && list.length > 0) ? list : (picture ? [picture] : [])
		let idx = -1
		if (picture) {
			idx = items.findIndex(item => item.id === picture.id)
			if (idx < 0) {
				images = [picture]
				idx = 0
			} else {
				images = items
			}
		} else {
			images = items
			idx = items.length > 0 ? 0 : -1
		}
		currentIndex = idx
		image = idx >= 0 ? images[idx] : picture
		resetView()
		forceActiveFocus()
	}

	function prev() {
		if (images.length <= 1)
			return
		currentIndex = (currentIndex - 1 + images.length) % images.length
		image = images[currentIndex]
		resetView()
	}

	function next() {
		if (images.length <= 1)
			return
		currentIndex = (currentIndex + 1) % images.length
		image = images[currentIndex]
		resetView()
	}

	function close() {
		if (image === null)
			return
		image = null
		images = []
		currentIndex = -1
		resetView()
		closed()
	}

	anchors.fill: parent
	visible: opacity > 0
	opacity: shown ? 1 : 0
	z: 100 // above everything of the pane

	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	Keys.onEscapePressed: close()
	Keys.onLeftPressed: prev()
	Keys.onRightPressed: next()
	Keys.onPressed: event => {
		if (event.key === Qt.Key_Plus || event.key === Qt.Key_Equal) {
			zoomBy(zoomStep)
			event.accepted = true
		} else if (event.key === Qt.Key_Minus || event.key === Qt.Key_Underscore) {
			zoomBy(1 / zoomStep)
			event.accepted = true
		} else if (event.key === Qt.Key_0) {
			resetView()
			event.accepted = true
		}
	}

	Rectangle {
		id: scrimRect
		anchors.fill: parent
		color: Qt.rgba(0, 0, 0, lightbox.scrimOpacity)

		TapHandler {
			onTapped: eventPoint => {
				const p = eventPoint.position
				if (zoomToolbar.contains(scrimRect.mapToItem(zoomToolbar, p)))
					return
				if (prevButton.visible && prevButton.contains(scrimRect.mapToItem(prevButton, p)))
					return
				if (nextButton.visible && nextButton.contains(scrimRect.mapToItem(nextButton, p)))
					return
				if (viewport.contains(scrimRect.mapToItem(viewport, p)) && stage.contains(scrimRect.mapToItem(stage, p)))
					return
				lightbox.close()
			}
		}
		MouseArea {
			anchors.fill: parent
			acceptedButtons: Qt.NoButton
			onWheel: wheel => {
				const delta = wheel.angleDelta.y !== 0 ? wheel.angleDelta.y : wheel.pixelDelta.y
				if (delta > 0)
					lightbox.zoomBy(lightbox.wheelZoomStep)
				else if (delta < 0)
					lightbox.zoomBy(1 / lightbox.wheelZoomStep)
				wheel.accepted = true
			}
		}
	}

	Item {
		id: viewport
		anchors {
			fill: parent
			margins: lightbox.margin
			bottomMargin: lightbox.margin + lightbox.toolbarHeight + lightbox.toolbarBottomGap + lightbox.captionHeight
		}
		clip: true
		scale: lightbox.shown ? 1 : lightbox.closedScale

		Behavior on scale { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		Item {
			id: stage
			x: (viewport.width - width) / 2 + lightbox.panX
			y: (viewport.height - height) / 2 + lightbox.panY
			width: picture.width * lightbox.zoom
			height: picture.height * lightbox.zoom

			// Not enlarged past its own size at 1x, one picture pixel to one device pixel.
			Image {
				id: picture
				anchors.centerIn: parent
				width: Math.min(viewport.width, implicitWidth / Screen.devicePixelRatio)
				height: Math.min(viewport.height, implicitHeight / Screen.devicePixelRatio)
				scale: lightbox.zoom
				asynchronous: true
				fillMode: Image.PreserveAspectFit
				smooth: lightbox.zoom !== 1.0
				source: lightbox.image && lightbox.image.id ? "image://chatimage/" + lightbox.sessionId + "/" + lightbox.image.id : ""
				cache: false
			}

			MouseArea {
				id: panArea
				anchors.fill: parent
				acceptedButtons: Qt.LeftButton
				cursorShape: lightbox.zoom > 1.0 ? (pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor) : Qt.ArrowCursor
				property real lastX: 0
				property real lastY: 0

				onPressed: mouse => {
					const pt = panArea.mapToItem(viewport, mouse.x, mouse.y)
					lastX = pt.x
					lastY = pt.y
					mouse.accepted = true
				}
				onPositionChanged: mouse => {
					if (!pressed || lightbox.zoom <= 1.0)
						return
					const pt = panArea.mapToItem(viewport, mouse.x, mouse.y)
					lightbox.panX += pt.x - lastX
					lightbox.panY += pt.y - lastY
					lastX = pt.x
					lastY = pt.y
				}
				onDoubleClicked: {
					if (Math.abs(lightbox.zoom - 1.0) > 0.01)
						lightbox.resetView()
					else
						lightbox.setZoom(lightbox.doubleClickZoom)
				}
			}
		}
	}

	CircleButton {
		id: prevButton
		objectName: "lightboxPrevButton"
		anchors { left: parent.left; leftMargin: lightbox.navMargin; verticalCenter: parent.verticalCenter }
		visible: lightbox.hasMultiple
		Accessible.name: qsTr("Previous image")
		onClicked: lightbox.prev()

		Chevron {
			anchors.centerIn: parent
			rotation: 180
			color: Theme.text
		}
	}

	CircleButton {
		id: nextButton
		objectName: "lightboxNextButton"
		anchors { right: parent.right; rightMargin: lightbox.navMargin; verticalCenter: parent.verticalCenter }
		visible: lightbox.hasMultiple
		Accessible.name: qsTr("Next image")
		onClicked: lightbox.next()

		Chevron {
			anchors.centerIn: parent
			rotation: 0
			color: Theme.text
		}
	}

	Rectangle {
		id: zoomToolbar
		objectName: "lightboxZoomToolbar"
		anchors { horizontalCenter: parent.horizontalCenter; bottom: caption.top; bottomMargin: lightbox.toolbarBottomGap }
		width: toolbarRow.implicitWidth + 12
		height: lightbox.toolbarHeight
		radius: height / 2
		color: Theme.surface
		border.color: Theme.border

		TapHandler {}

		Row {
			id: toolbarRow
			anchors.centerIn: parent
			spacing: 4

			IconButton {
				objectName: "lightboxZoomOutButton"
				buttonSize: 28
				iconName: "minus"
				iconSize: 16
				enabled: lightbox.zoom > lightbox.minZoom
				opacity: enabled ? 1 : Theme.disabledOpacity
				tooltipText: qsTr("Zoom out")
				Accessible.name: qsTr("Zoom out")
				onClicked: lightbox.zoomBy(1 / lightbox.zoomStep)
			}

			TextButton {
				objectName: "lightboxZoomResetButton"
				anchors.verticalCenter: parent.verticalCenter
				text: Math.round(lightbox.zoom * 100) + "%"
				Accessible.name: qsTr("Reset zoom")
				onClicked: lightbox.resetView()
			}

			IconButton {
				objectName: "lightboxZoomInButton"
				buttonSize: 28
				iconName: "plus"
				iconSize: 16
				enabled: lightbox.zoom < lightbox.maxZoom
				opacity: enabled ? 1 : Theme.disabledOpacity
				tooltipText: qsTr("Zoom in")
				Accessible.name: qsTr("Zoom in")
				onClicked: lightbox.zoomBy(lightbox.zoomStep)
			}
		}
	}

	Text {
		id: caption
		objectName: "lightboxCaption"
		textFormat: Text.PlainText
		anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: lightbox.margin / 2 }
		text: lightbox.captionText
		color: Qt.rgba(1, 1, 1, 0.8)
		font.pixelSize: Theme.smallFontSize
	}

	IconButton {
		anchors { top: parent.top; right: parent.right; margins: 12 }
		iconName: "close"
		Accessible.name: qsTr("Close")
		onClicked: lightbox.close()
	}
}
