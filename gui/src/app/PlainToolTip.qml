import QtQuick
import QtQuick.Controls.Basic

// A tooltip whose text is shown as it is. The default one reads rich text, and file names
// or model names are not ours to say what they mean.
ToolTip {
	id: tip

	contentItem: Text {
		text: tip.text
		textFormat: Text.PlainText
		font: tip.font
		wrapMode: Text.Wrap
		color: tip.palette.toolTipText
	}
}
