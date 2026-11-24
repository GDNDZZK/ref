pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Lightweight structural placeholder for an unloaded or evicted message.
Item {
	id: root

	required property string messageRole
	required property var skeletonParts
	required property real estimatedHeight
	required property bool opensTurn
	required property bool stale
	required property int index

	readonly property var chat: ListView.view
	readonly property real columnWidth: chat ? chat.columnWidth : Theme.contentWidth
	readonly property int lineHeight: 10
	readonly property int lineGap: 8

	width: chat ? chat.width : Theme.contentWidth
	height: Math.max(40, estimatedHeight)
	opacity: stale ? Theme.staleOpacity : 1

	Item {
		id: column
		x: (root.width - root.columnWidth) / 2
		width: root.columnWidth
		height: parent.height
		clip: true

		Rectangle {
			id: turnDivider
			visible: root.opensTurn
			y: Math.round(Theme.turnGap / 2)
			width: parent.width
			height: 1
			color: Theme.border
		}

		// User message bubble skeleton
		Rectangle {
			visible: root.messageRole === "user"
			anchors.top: root.opensTurn ? turnDivider.bottom : parent.top
			anchors.topMargin: root.opensTurn ? Math.round(Theme.turnGap / 2) : 0
			width: Math.min(parent.width, Math.max(160, parent.width * 0.62))
			height: Math.max(38, Math.min(parent.height - (root.opensTurn ? Theme.turnGap + 8 : 8), 96))
			radius: Theme.radius
			color: Theme.userBubble
			border.width: 1
			border.color: Theme.userBubbleBorder

			Column {
				anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 14 }
				spacing: root.lineGap

				Rectangle {
					width: parent.width * 0.85
					height: root.lineHeight
					radius: height / 2
					color: Theme.border
					opacity: 0.7
				}
				Rectangle {
					visible: parent.parent.height > 54
					width: parent.width * 0.55
					height: root.lineHeight
					radius: height / 2
					color: Theme.border
					opacity: 0.55
				}
			}
		}

		// Assistant structured skeleton
		Column {
			visible: root.messageRole !== "user"
			width: parent.width
			spacing: 10

			Repeater {
				model: root.skeletonParts

				delegate: Item {
					id: partItem
					required property var modelData
					readonly property string kind: modelData.kind ?? "text"
					readonly property string toolName: modelData.tool ?? ""
					readonly property int linesCount: Math.max(1, Math.min(4, modelData.lines ?? 2))
					readonly property int codeBlocks: modelData.codeBlocks ?? 0
					readonly property int tables: modelData.tables ?? 0
					readonly property int imgW: modelData.width ?? 0
					readonly property int imgH: modelData.height ?? 0

					width: column.width
					height: partCol.implicitHeight

					Column {
						id: partCol
						width: parent.width
						spacing: 8

						// Reasoning collapsed bar
						Rectangle {
							visible: partItem.kind === "reasoning"
							width: Math.min(parent.width, 160)
							height: 28
							radius: Theme.radiusSmall
							color: Theme.surface
							border.width: 1
							border.color: Theme.border

							Rectangle {
								anchors { left: parent.left; leftMargin: 12; verticalCenter: parent.verticalCenter }
								width: 84
								height: 8
								radius: 4
								color: Theme.border
							}
						}

						// Text lines + optional code/table card
						Column {
							visible: partItem.kind === "text"
							width: parent.width
							spacing: root.lineGap

							Repeater {
								model: partItem.kind === "text" ? partItem.linesCount : 0

								delegate: Rectangle {
									required property int index
									readonly property var widths: [0.92, 0.84, 0.74, 0.52]
									width: partCol.width * (widths[index % widths.length])
									height: root.lineHeight
									radius: height / 2
									color: Theme.surfaceHover
								}
							}

							Rectangle {
								visible: partItem.codeBlocks > 0 || partItem.tables > 0
								width: parent.width
								height: 54
								radius: Theme.radius
								color: Theme.codeBackground
								border.width: 1
								border.color: Theme.border

								Column {
									anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 12 }
									spacing: 8

									Rectangle { width: parent.width * 0.5; height: 8; radius: 4; color: Theme.border }
									Rectangle { width: parent.width * 0.72; height: 8; radius: 4; color: Theme.border; opacity: 0.7 }
								}
							}
						}

						// Tool pill or todo checklist
						Rectangle {
							visible: partItem.kind === "tool" && partItem.toolName !== "todo"
							width: parent.width
							height: 38
							radius: Theme.radius
							color: Theme.surface
							border.width: 1
							border.color: Theme.border

							Row {
								anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 12; rightMargin: 12 }
								spacing: 8

								Rectangle {
									anchors.verticalCenter: parent.verticalCenter
									width: 8
									height: 8
									radius: 4
									color: Theme.border
								}
								Text {
									textFormat: Text.PlainText
									anchors.verticalCenter: parent.verticalCenter
									text: partItem.toolName !== "" ? partItem.toolName : qsTr("tool")
									color: Theme.textMuted
									font.pixelSize: Theme.smallFontSize
									elide: Text.ElideRight
								}
								Rectangle {
									anchors.verticalCenter: parent.verticalCenter
									width: Math.max(40, Math.min(180, parent.width * 0.35))
									height: 8
									radius: 4
									color: Theme.border
									opacity: 0.7
								}
							}
						}

						Rectangle {
							visible: partItem.kind === "tool" && partItem.toolName === "todo"
							width: parent.width
							height: 82
							radius: Theme.radius
							color: Theme.surface
							border.width: 1
							border.color: Theme.border

							Column {
								anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 12 }
								spacing: 8

								Repeater {
									model: 3
									delegate: Row {
										required property int index
										spacing: 8
										Rectangle { width: 12; height: 12; radius: 3; color: Theme.border }
										Rectangle { anchors.verticalCenter: parent.verticalCenter; width: 140 + index * 32; height: 8; radius: 4; color: Theme.border }
									}
								}
							}
						}

						// Image aspect box
						Rectangle {
							visible: partItem.kind === "image"
							readonly property real boxWidth: partItem.imgW > 0 ? Math.min(parent.width, Math.min(320, partItem.imgW)) : 120
							readonly property real boxHeight: (partItem.imgW > 0 && partItem.imgH > 0)
								? Math.max(72, Math.min(220, boxWidth * partItem.imgH / partItem.imgW))
								: 72
							width: boxWidth
							height: boxHeight
							radius: Theme.radius
							color: Theme.surface
							border.width: 1
							border.color: Theme.border
						}
					}
				}
			}
		}
	}
}
