import QtQuick
import RefGui

// The handle of a scroll bar in the theme's colors. The bar sets the length; the owner sets the side that is `thickness` wide.
Rectangle {
	property bool pressed: false
	readonly property int thickness: 6

	radius: thickness / 2
	color: pressed ? Theme.scrollHandlePressed : Theme.scrollHandle
}
