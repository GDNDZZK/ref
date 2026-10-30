import QtQuick
import RefGui

// A line icon from assets/icons in one colour. Always shown at its grid size (16 or 20 logical
// pixels, never scaled): the strokes are aligned to device pixels at that size only. Place it
// at whole logical pixels, for example by centring in an even-sized parent.
Image {
	id: icon

	required property string name // File name without the set suffix, e.g. "check"
	property int size: 16 // The set: 16 or 20
	property color color: Theme.text

	// Hex without '#': the provider takes it from the URL, where '#' would start a fragment.
	readonly property string colorHex: String(color).replace("#", "")

	width: size
	height: size
	// Rasterised at device pixels so that the image maps 1:1 onto the screen.
	sourceSize: Qt.size(size * Screen.devicePixelRatio, size * Screen.devicePixelRatio)
	source: "image://icons/" + name + "-" + size + "?color=" + colorHex
	fillMode: Image.Pad
	smooth: false
}
