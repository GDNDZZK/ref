import QtQuick
import RefGui

// Segmented control: `model` is a list of {text, value}; `value` is the chosen one. The
// indicator slides to the chosen segment.
Item {
	id: segmented

	property var model: []
	property var value
	readonly property int currentIndex: {
		for (let i = 0; i < model.length; ++i) {
			if (model[i].value === value)
				return i
		}
		return -1
	}

	signal activated(var value)

	implicitWidth: 300
	implicitHeight: 30

	Rectangle {
		anchors.fill: parent
		radius: height / 2
		color: Theme.surface
		border.color: Theme.border
	}

	Rectangle {
		readonly property real segment: (segmented.width - 4) / Math.max(1, segmented.model.length)
		x: 2 + Math.max(0, segmented.currentIndex) * segment
		y: 2
		width: segment
		height: segmented.height - 4
		radius: height / 2
		color: Theme.userBubble
		border.color: Theme.userBubbleBorder
		visible: segmented.currentIndex >= 0

		Behavior on x { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
		Behavior on width { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	Row {
		x: 2
		y: 2
		width: parent.width - 4
		height: parent.height - 4

		Repeater {
			model: segmented.model

			Item {
				required property var modelData
				required property int index
				width: parent.width / segmented.model.length
				height: parent.height

				Text {
					textFormat: Text.PlainText
					anchors.centerIn: parent
					text: parent.modelData.text
					color: parent.index === segmented.currentIndex ? Theme.text : Theme.textMuted
					font.pixelSize: Theme.smallFontSize + 1

					Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: segmented.activated(parent.modelData.value) }
			}
		}
	}
}
