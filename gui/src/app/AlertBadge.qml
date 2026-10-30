import QtQuick
import Qutie

// "!" in a disc, drawn rather than typed so it centers exactly at any size.
Rectangle {
	id: badge

	property color glyphColor: Theme.onAccent

	readonly property real stroke: Math.max(2, Math.round(width / 9))

	implicitWidth: 18
	implicitHeight: 18
	radius: width / 2
	color: Theme.danger

	Rectangle {
		anchors.horizontalCenter: parent.horizontalCenter
		y: Math.round(badge.height * 0.22)
		width: badge.stroke
		height: Math.round(badge.height * 0.39)
		radius: width / 2
		color: badge.glyphColor
	}
	Rectangle {
		anchors.horizontalCenter: parent.horizontalCenter
		y: Math.round(badge.height * 0.67)
		width: badge.stroke
		height: badge.stroke
		radius: width / 2
		color: badge.glyphColor
	}
}
