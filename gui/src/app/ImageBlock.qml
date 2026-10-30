import QtQuick
import RefGui

// The pictures attached to a message, under its text.
Block {
	id: block

	required property var images

	ImageStrip {
		images: block.images
		sessionId: block.chat.sessionId
		onImageClicked: image => block.chat.imageClicked(image)
		maxWidth: block.thumbnailMaxWidth
		maxHeight: block.thumbnailHeight
	}

	readonly property int thumbnailHeight: 72
	readonly property int thumbnailMaxWidth: 240
}
