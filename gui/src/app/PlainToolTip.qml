import QtQuick
import QtQuick.Controls.Basic
import RefGui

// A tooltip whose text is shown as it is. The default one reads rich text, and file names
// or model names are not ours to say what they mean.
ToolTip {
	id: tip

	readonly property int maxContentWidth: 420
	readonly property int padX: 10
	readonly property int padY: 6
	readonly property real uiScale: UiScale.factor

	padding: 0
	font.pixelSize: Theme.smallFontSize
	implicitWidth: tipInner.width * uiScale
	implicitHeight: tipInner.height * uiScale
	x: parent ? (parent.width - tipInner.width) / 2 : 0
	y: -tipInner.height - 3
	background: null

	contentItem: Item {
		implicitWidth: tipInner.width * tip.uiScale
		implicitHeight: tipInner.height * tip.uiScale

		Item {
			id: tipInner
			readonly property real textWidth: Math.min(tipText.implicitWidth, tip.maxContentWidth)
			width: textWidth + 2 * tip.padX
			height: tipText.implicitHeight + 2 * tip.padY
			transformOrigin: Item.TopLeft
			scale: tip.uiScale

			Rectangle {
				anchors.fill: parent
				radius: Theme.radiusSmall
				color: Theme.surface
				border.color: Theme.border
			}

			Text {
				id: tipText
				x: tip.padX
				y: tip.padY
				width: tipInner.textWidth
				text: tip.text
				textFormat: Text.PlainText
				font: tip.font
				wrapMode: Text.Wrap
				color: Theme.text
			}
		}
	}
}
