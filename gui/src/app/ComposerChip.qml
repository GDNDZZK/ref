import QtQuick
import RefGui

// A pill in the tag row: an icon and a label. When the label changes, the old text slides up and
// fades out while the new one rises in; the pill's width follows the new text.
Rectangle {
	id: chip

	readonly property int chipHeight: 28
	readonly property int paddingX: 10
	readonly property int iconGap: 6
	readonly property int labelSlide: 6
	readonly property int maxWidth: 240
	readonly property int iconSize: 16
	readonly property int tipDelay: 600

	property string iconName
	property color iconColor: Theme.textMuted
	property color labelColor: hover.hovered && clickable ? Theme.text : Theme.textMuted
	property string label
	property string tip
	property bool clickable: true
	// The pill is absent while there is nothing to say.
	readonly property bool shown: label !== ""
	signal clicked()

	readonly property real naturalWidth: 2 * paddingX + iconSize + iconGap + current.implicitWidth

	implicitWidth: shown ? Math.min(naturalWidth, maxWidth) : 0
	implicitHeight: chipHeight
	radius: height / 2
	color: hover.hovered && clickable ? Theme.surfaceHover : "transparent"
	opacity: shown ? 1 : 0
	visible: opacity > 0 || width > 0
	clip: true

	Behavior on implicitWidth { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

	onLabelChanged: {
		if (current.text !== "" && label !== "") {
			previous.text = current.text
			morph.restart()
		}
		current.text = label
	}
	Component.onCompleted: current.text = label

	Icon {
		id: icon
		x: chip.paddingX
		y: (chip.height - height) / 2
		name: chip.iconName
		size: chip.iconSize
		color: chip.iconColor

		Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	Item {
		id: labelBox
		x: chip.paddingX + chip.iconSize + chip.iconGap
		width: Math.max(0, chip.width - x - chip.paddingX)
		height: chip.height
		clip: true

		Text {
			id: current
			textFormat: Text.PlainText
			property real slide: 0
			y: (parent.height - height) / 2 + slide
			width: parent.width
			elide: Text.ElideRight
			color: chip.labelColor
			font.pixelSize: Theme.smallFontSize

			Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		}

		Text {
			id: previous
			textFormat: Text.PlainText
			property real slide: 0
			y: (parent.height - height) / 2 + slide
			width: parent.width
			elide: Text.ElideRight
			color: chip.labelColor
			font.pixelSize: Theme.smallFontSize
			opacity: 0
		}
	}

	ParallelAnimation {
		id: morph
		NumberAnimation { target: previous; property: "opacity"; from: 1; to: 0; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: previous; property: "slide"; from: 0; to: -chip.labelSlide; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: current; property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
		NumberAnimation { target: current; property: "slide"; from: chip.labelSlide; to: 0; duration: Motion.standard; easing.type: Motion.ease }
	}

	HoverHandler {
		id: hover
		cursorShape: chip.clickable ? Qt.PointingHandCursor : Qt.ArrowCursor
	}
	TapHandler {
		enabled: chip.clickable
		onTapped: chip.clicked()
	}

	PlainToolTip {
		visible: hover.hovered && chip.tip !== ""
		text: chip.tip
		delay: chip.tipDelay
	}

	Accessible.role: clickable ? Accessible.Button : Accessible.StaticText
	Accessible.name: label
	Accessible.description: tip
	Accessible.onPressAction: clicked()
}
