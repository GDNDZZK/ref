import QtQuick
import RefGui

// The first row of a conversation that has earlier messages on the backend: a button that
// loads them. It shows a pulsing dot while loading and the reason when the last load failed.
Item {
	id: pill

	property bool loading: false
	property string error: ""

	signal clicked()

	readonly property int pillHeight: 28
	readonly property int innerPadding: 12
	readonly property int maxWidth: 480

	implicitWidth: parent ? parent.width : 0
	implicitHeight: pillHeight

	Rectangle {
		id: face
		anchors.horizontalCenter: parent.horizontalCenter
		width: Math.min(row.width + 2 * pill.innerPadding, pill.maxWidth)
		height: pill.pillHeight
		radius: height / 2
		color: hover.hovered && !pill.loading ? Theme.surfaceHover : Theme.surface
		border.color: pill.error !== "" ? Qt.alpha(Theme.danger, 0.5) : Theme.border

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Row {
			id: row
			anchors.centerIn: parent
			spacing: 6

			Item {
				anchors.verticalCenter: parent.verticalCenter
				width: 16
				height: 16

				Rectangle {
					id: dot
					anchors.centerIn: parent
					width: 8
					height: 8
					radius: 4
					color: Theme.accent
					visible: pill.loading

					Pulse { target: dot; running: pill.loading; dimmed: 0.5 }
				}
				Icon {
					name: "history"
					color: Theme.textMuted
					visible: !pill.loading
				}
			}
			Text {
				anchors.verticalCenter: parent.verticalCenter
				width: Math.min(implicitWidth, pill.maxWidth - 2 * pill.innerPadding - 22)
				text: pill.error !== "" ? pill.error : qsTr("Load older messages")
				elide: Text.ElideRight
				color: pill.error !== "" ? Theme.danger : Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}
		}
	}

	HoverHandler { id: hover; cursorShape: pill.loading ? Qt.ArrowCursor : Qt.PointingHandCursor }
	TapHandler {
		enabled: !pill.loading
		onTapped: pill.clicked()
	}
	Accessible.role: Accessible.Button
	Accessible.name: qsTr("Load older messages")
}
