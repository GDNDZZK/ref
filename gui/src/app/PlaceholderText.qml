import QtQuick
import RefGui

// The hint that shows in a TextInput while it is empty. The owner places it (usually anchors.fill).
Text {
	required property TextInput textInput

	textFormat: Text.PlainText
	verticalAlignment: Text.AlignVCenter
	color: Theme.textMuted
	font.pixelSize: Theme.fontSize
	visible: textInput.text === "" && textInput.preeditText === ""
}
