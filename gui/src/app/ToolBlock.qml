// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import QtQuick.Layouts
import RefGui

// A tool call: its state, what it does and, on demand, its input and output. A call that
// waits for permission opens and asks for it in the card; one that waits to start can be
// cancelled from the card.
Block {
	id: block

	required property string toolName
	required property string purpose
	required property string toolState
	required property string inputJson
	required property string rawInput // the arguments while they stream
	required property string output
	required property bool failed
	required property string callId
	required property real time
	required property real timeEnd
	required property real runAt
	required property bool expanded
	required property bool showAll
	required property var meta
	required property string liveOutput
	required property var images

	readonly property var inputObject: ToolInfo.parse(inputJson)
	readonly property string detail: ToolInfo.detail(toolName, inputObject)
	readonly property bool hasDetails: !alwaysOpen && (inputJson !== "" || rawInput !== "" || output !== "" || images.length > 0)
	readonly property bool awaitingPermission: toolState === "awaiting_permission"
	readonly property bool delayed: toolState === "delayed"
	readonly property bool alwaysOpen: toolName === "todo" && ToolInfo.todos(inputObject).length > 0
	readonly property bool open: alwaysOpen || ((expanded || awaitingPermission) && hasDetails)
	readonly property bool inactive: toolState === "denied" || toolState === "cancelled"
	readonly property bool showsTime: columnWidth >= timeShownFromWidth
	readonly property int timeShownFromWidth: 560
	readonly property int headerHeight: 38
	readonly property int cardPadding: 14
	readonly property int itemSpacing: 10
	readonly property real attentionBorderOpacity: 0.6
	readonly property int tickInterval: 250 // ms between redraws of the clock texts
	readonly property string firstOutputLine: ToolInfo.firstLine(output)

	property real now: Date.now()

	// What the right end of the header says, and in which color.
	readonly property string statusText: {
		const took = timeEnd > 0 ? Format.duration(timeEnd - time) : ""
		switch (toolState) {
		case "streaming":
			return qsTr("writing\u2026")
		case "awaiting_permission":
			return qsTr("Needs permission")
		case "delayed":
			return qsTr("Runs in %1").arg(Format.elapsed(Math.max(0, runAt - now) + Format.msPerSecond - 1))
		case "running":
			return qsTr("running %1").arg(Format.elapsed(now - time))
		case "error":
			return took === "" ? qsTr("error") : qsTr("error %1").arg(took)
		case "denied":
			return qsTr("denied")
		case "cancelled":
			return qsTr("cancelled")
		default:
			return took
		}
	}
	readonly property color statusColor: {
		switch (toolState) {
		case "awaiting_permission":
		case "delayed":
			return Theme.warning
		case "streaming":
		case "running":
			return Theme.accent
		case "error":
			return Theme.danger
		default:
			return Theme.textMuted
		}
	}

	Timer {
		interval: block.tickInterval
		running: block.toolState === "running" || block.delayed
		repeat: true
		onTriggered: block.now = Date.now()
	}

	Rectangle {
		id: card
		width: block.columnWidth
		height: header.height + failure.height + body.height + permission.height
		radius: Theme.radius
		color: hover.hovered && block.hasDetails ? Theme.surfaceHover : Theme.surface
		border.color: block.awaitingPermission || block.delayed ? Qt.alpha(Theme.warning, block.attentionBorderOpacity) : Theme.border

		Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
		Behavior on border.color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }

		// Stays in view while the output scrolls past under it (Block.pinOffset).
		Item {
			id: header

			readonly property real offset: block.pinOffset(height)

			y: offset
			z: 1 // above the output
			width: parent.width
			height: block.headerHeight

			// Hides the output under the pinned header. Inside the card's border; its
			// corners follow the card's where the header nears the card's top or end.
			Rectangle {
				readonly property real inner: card.radius - card.border.width
				readonly property real toEnd: card.height - header.offset - header.height

				anchors {
					fill: parent
					leftMargin: card.border.width
					rightMargin: card.border.width
					topMargin: Math.max(0, card.border.width - header.offset)
					bottomMargin: Math.max(0, card.border.width - toEnd)
				}
				topLeftRadius: Math.max(0, inner - header.offset)
				topRightRadius: topLeftRadius
				bottomLeftRadius: Math.max(0, inner - toEnd)
				bottomRightRadius: bottomLeftRadius
				color: card.color

				Rectangle {
					anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
					height: 1
					color: Theme.border
					visible: header.offset > 0 && parent.toEnd > 0 // at rest, the body's divider is here
				}
			}

			HoverHandler {
				id: hover
				cursorShape: block.hasDetails && !block.awaitingPermission ? Qt.PointingHandCursor : Qt.ArrowCursor
			}
			TapHandler {
				enabled: block.hasDetails && !block.awaitingPermission
				onTapped: {
					if (block.open && header.offset > 0)
						block.holdHeader(header.height)
					block.toggleExpanded()
				}
			}

			RowLayout {
				anchors { fill: parent; leftMargin: block.cardPadding; rightMargin: block.cardPadding }
				spacing: block.itemSpacing

				ToolStateSlot {
					toolState: block.toolState
					runAt: block.runAt
				}
				Icon {
					name: ToolInfo.icon(block.toolName)
					color: Theme.textMuted
				}
				Text {
					textFormat: Text.PlainText
					text: block.toolName
					color: block.inactive ? Theme.textMuted : Theme.text
					font.family: Theme.monoFamily
					font.pixelSize: Theme.codeFontSize
					font.bold: true
					font.strikeout: block.toolState === "denied"
				}
				// The purpose keeps its words; the detail gets what is left.
				Item {
					Layout.fillWidth: true
					Layout.preferredWidth: 0
					Layout.fillHeight: true

					Text {
						textFormat: Text.PlainText
						id: purposeText
						anchors.verticalCenter: parent.verticalCenter
						width: Math.min(implicitWidth, parent.width)
						text: block.purpose
						elide: Text.ElideRight
						color: block.toolState === "cancelled" ? Theme.textMuted : Theme.text
						font.pixelSize: Theme.codeFontSize
					}
					Text {
						textFormat: Text.PlainText
						anchors.verticalCenter: parent.verticalCenter
						x: purposeText.visible && purposeText.text !== "" ? purposeText.width + block.itemSpacing : 0
						width: Math.max(0, parent.width - x)
						text: block.detail
						elide: Text.ElideRight
						color: Theme.textMuted
						font.family: Theme.monoFamily
						font.pixelSize: Theme.smallFontSize
					}
				}
				TextButton {
					visible: block.delayed
					text: qsTr("Cancel")
					onClicked: block.ListView.view.cancelRequested()
				}
				Text {
					textFormat: Text.PlainText
					text: block.statusText
					color: block.statusColor
					font.pixelSize: Theme.smallFontSize

					Behavior on color { ColorAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
				Text {
					textFormat: Text.PlainText
					visible: block.showsTime
					text: Format.clock(block.time)
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}
				Chevron {
					rotation: block.open ? 90 : 0
					opacity: block.hasDetails ? 1 : 0

					Behavior on rotation { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
					Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				}
			}
		}

		// Why a failed call failed, without opening the card.
		Item {
			id: failure

			readonly property bool shown: block.toolState === "error" && !block.open && block.firstOutputLine !== ""

			y: header.height
			width: parent.width
			height: shown ? 26 : 0
			clip: true

			Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: block.cardPadding; right: parent.right; rightMargin: block.cardPadding; top: parent.top }
				text: "\u2514 " + block.firstOutputLine
				elide: Text.ElideRight
				color: Theme.danger
				font.family: Theme.monoFamily
				font.pixelSize: Theme.smallFontSize
			}
		}

		Item {
			id: body
			y: header.height + failure.height
			width: parent.width
			height: block.open && bodyLoader.item ? bodyLoader.item.height : 0
			clip: true

			Behavior on height {
				NumberAnimation {
					duration: Motion.standard
					easing.type: Motion.ease
					onRunningChanged: if (!running) block.releaseHeader()
				}
			}

			// While the body is shorter than the card's corner radius, the divider lies
			// within the rounded bottom corners; inset it to the arc so its ends do not
			// stick out past them. Collapsed, it matches the straight part of the bottom edge.
			Rectangle {
				readonly property real cornerRadius: Theme.radius
				readonly property real belowCornerCenter: Math.max(0, cornerRadius - body.height)
				readonly property real inset: cornerRadius - Math.sqrt(cornerRadius * cornerRadius - belowCornerCenter * belowCornerCenter)

				x: inset
				width: parent.width - 2 * inset
				height: 1
				color: Theme.border
			}

			// Kept while the body folds away, so it does not vanish before it closes.
			Loader {
				id: bodyLoader
				width: parent.width
				active: block.open || body.height > 0

				sourceComponent: ToolBody {
					call: block
					onShowAllRequested: block.ListView.view.model.toggleShowAll(block.index)
				}
			}
		}

		PermissionSection {
			id: permission
			y: header.height + failure.height + body.height
			toolName: block.toolName
			input: block.inputObject
			open: block.awaitingPermission
			othersWaiting: block.ListView.view.permissionCount - 1
			onAnswered: (allow, always) => block.ListView.view.permissionAnswered(block.callId, allow, always)
		}
	}
}
