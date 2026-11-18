import QtQuick
import QtQuick.Controls.Basic
import RefGui

// A tooltip whose text is shown as it is. The default one reads rich text, and file names
// or model names are not ours to say what they mean.
ToolTip {
	id: tip

	readonly property int maxContentWidth: 420

	leftPadding: 10
	rightPadding: 10
	topPadding: 6
	bottomPadding: 6
	font.pixelSize: Theme.smallFontSize
	implicitWidth: Math.min(tipText.implicitWidth, maxContentWidth) + leftPadding + rightPadding

	contentItem: Text {
		id: tipText
		text: tip.text
		textFormat: Text.PlainText
		font: tip.font
		wrapMode: Text.Wrap
		color: Theme.text
	}

	background: Rectangle {
		radius: Theme.radiusSmall
		color: Theme.surface
		border.color: Theme.border
	}
}
