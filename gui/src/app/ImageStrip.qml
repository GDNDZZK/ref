import QtQuick
import RefGui

// The pictures of a message or tool call in a row, wrapping when the width runs out.
Flow {
	id: strip

	required property var images
	property string sessionId

	signal imageClicked(var image)

	property int maxWidth: 160
	property int maxHeight: 96

	spacing: 8

	Repeater {
		model: strip.images

		delegate: Thumbnail {
			required property var modelData

			sessionId: strip.sessionId
			image: modelData
			maxWidth: strip.maxWidth
			maxHeight: strip.maxHeight
			onClicked: strip.imageClicked(modelData)
		}
	}
}
