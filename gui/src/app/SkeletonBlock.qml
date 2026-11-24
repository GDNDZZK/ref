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
	readonly property int minHeight: 40
	readonly property int lineHeight: 10
	readonly property int barHeight: 8
	readonly property int lineGap: 8
	readonly property int partGap: 10
	readonly property int cardInset: 12
	readonly property int userInset: 14
	readonly property int userMinWidth: 160
	readonly property real userWidthRatio: 0.62
	readonly property int userMinHeight: 38
	readonly property int userMaxHeight: 96
	readonly property int userVerticalMargin: 8
	readonly property int userTwoLineThreshold: 54
	readonly property real userLineOneRatio: 0.85
	readonly property real userLineTwoRatio: 0.55
	readonly property real lineOpacityPrimary: 0.7
	readonly property real lineOpacitySecondary: 0.55
	readonly property int maxProseLines: 4
	readonly property int defaultProseLines: 2
	readonly property var proseLineRatios: [0.92, 0.84, 0.74, 0.52]
	readonly property int reasoningWidth: 160
	readonly property int reasoningHeight: 28
	readonly property int reasoningBarWidth: 84
	readonly property int codeCardHeight: 54
	readonly property real codeLineOneRatio: 0.5
	readonly property real codeLineTwoRatio: 0.72
	readonly property int toolPillHeight: 38
	readonly property int toolBarMinWidth: 40
	readonly property int toolBarMaxWidth: 180
	readonly property real toolBarWidthRatio: 0.35
	readonly property int todoCardHeight: 82
	readonly property int todoRowCount: 3
	readonly property int todoCheckSize: 12
	readonly property int todoCheckRadius: 3
	readonly property int todoBarBaseWidth: 140
	readonly property int todoBarStepWidth: 32
	readonly property int imageMaxWidth: 320
	readonly property int imageFallbackWidth: 120
	readonly property int imageMinHeight: 72
	readonly property int imageMaxHeight: 220

	width: chat ? chat.width : Theme.contentWidth
	height: Math.max(minHeight, estimatedHeight)
	opacity: stale ? Theme.staleOpacity : 1

	component UserSkeleton: Rectangle {
		anchors.top: root.opensTurn ? turnDivider.bottom : parent.top
		anchors.topMargin: root.opensTurn ? Math.round(Theme.turnGap / 2) : 0
		width: Math.min(parent.width, Math.max(root.userMinWidth, parent.width * root.userWidthRatio))
		height: Math.max(root.userMinHeight, Math.min(parent.height - (root.opensTurn ? Theme.turnGap + root.userVerticalMargin : root.userVerticalMargin), root.userMaxHeight))
		radius: Theme.radius
		color: Theme.userBubble
		border.width: 1
		border.color: Theme.userBubbleBorder

		Column {
			anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: root.userInset }
			spacing: root.lineGap

			Rectangle {
				width: parent.width * root.userLineOneRatio
				height: root.lineHeight
				radius: height / 2
				color: Theme.border
				opacity: root.lineOpacityPrimary
			}
			Rectangle {
				visible: parent.parent.height > root.userTwoLineThreshold
				width: parent.width * root.userLineTwoRatio
				height: root.lineHeight
				radius: height / 2
				color: Theme.border
				opacity: root.lineOpacitySecondary
			}
		}
	}

	component ReasoningSkeleton: Rectangle {
		width: Math.min(parent.width, root.reasoningWidth)
		height: root.reasoningHeight
		radius: Theme.radiusSmall
		color: Theme.surface
		border.width: 1
		border.color: Theme.border

		Rectangle {
			anchors { left: parent.left; leftMargin: root.cardInset; verticalCenter: parent.verticalCenter }
			width: root.reasoningBarWidth
			height: root.barHeight
			radius: height / 2
			color: Theme.border
		}
	}

	component CodeCardSkeleton: Rectangle {
		height: root.codeCardHeight
		radius: Theme.radius
		color: Theme.codeBackground
		border.width: 1
		border.color: Theme.border

		Column {
			anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: root.cardInset }
			spacing: root.lineGap

			Rectangle { width: parent.width * root.codeLineOneRatio; height: root.barHeight; radius: height / 2; color: Theme.border }
			Rectangle { width: parent.width * root.codeLineTwoRatio; height: root.barHeight; radius: height / 2; color: Theme.border; opacity: root.lineOpacityPrimary }
		}
	}

	component TextSkeleton: Column {
		id: textCol
		required property int linesCount
		required property bool hasRichCard
		spacing: root.lineGap

		Repeater {
			model: textCol.linesCount
			delegate: Rectangle {
				required property int index
				width: textCol.width * root.proseLineRatios[index % root.proseLineRatios.length]
				height: root.lineHeight
				radius: height / 2
				color: Theme.surfaceHover
			}
		}

		CodeCardSkeleton {
			visible: textCol.hasRichCard
			width: parent.width
		}
	}

	component ToolSkeleton: Rectangle {
		id: toolCard
		required property string toolName
		height: root.toolPillHeight
		radius: Theme.radius
		color: Theme.surface
		border.width: 1
		border.color: Theme.border

		Row {
			anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: root.cardInset; rightMargin: root.cardInset }
			spacing: root.lineGap

			Rectangle {
				anchors.verticalCenter: parent.verticalCenter
				width: root.barHeight
				height: root.barHeight
				radius: height / 2
				color: Theme.border
			}
			Text {
				textFormat: Text.PlainText
				anchors.verticalCenter: parent.verticalCenter
				text: toolCard.toolName !== "" ? toolCard.toolName : qsTr("tool")
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
				elide: Text.ElideRight
			}
			Rectangle {
				anchors.verticalCenter: parent.verticalCenter
				width: Math.max(root.toolBarMinWidth, Math.min(root.toolBarMaxWidth, parent.width * root.toolBarWidthRatio))
				height: root.barHeight
				radius: height / 2
				color: Theme.border
				opacity: root.lineOpacityPrimary
			}
		}
	}

	component TodoSkeletonRow: Row {
		required property int index
		spacing: root.lineGap

		Rectangle { width: root.todoCheckSize; height: root.todoCheckSize; radius: root.todoCheckRadius; color: Theme.border }
		Rectangle {
			anchors.verticalCenter: parent.verticalCenter
			width: root.todoBarBaseWidth + index * root.todoBarStepWidth
			height: root.barHeight
			radius: height / 2
			color: Theme.border
		}
	}

	component TodoSkeleton: Rectangle {
		height: root.todoCardHeight
		radius: Theme.radius
		color: Theme.surface
		border.width: 1
		border.color: Theme.border

		Column {
			anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: root.cardInset }
			spacing: root.lineGap

			Repeater {
				model: root.todoRowCount
				delegate: TodoSkeletonRow {}
			}
		}
	}

	component ImageSkeleton: Rectangle {
		required property int imgW
		required property int imgH
		required property real maxParentWidth
		readonly property real boxWidth: imgW > 0 ? Math.min(maxParentWidth, Math.min(root.imageMaxWidth, imgW)) : root.imageFallbackWidth
		readonly property real boxHeight: (imgW > 0 && imgH > 0)
			? Math.max(root.imageMinHeight, Math.min(root.imageMaxHeight, boxWidth * imgH / imgW))
			: root.imageMinHeight
		width: boxWidth
		height: boxHeight
		radius: Theme.radius
		color: Theme.surface
		border.width: 1
		border.color: Theme.border
	}

	component PartSkeleton: Column {
		id: partCol
		required property var modelData
		readonly property string kind: modelData.kind ?? "text"
		readonly property string toolName: modelData.tool ?? ""
		readonly property int linesCount: Math.max(1, Math.min(root.maxProseLines, modelData.lines ?? root.defaultProseLines))
		readonly property bool hasRichCard: (modelData.codeBlocks ?? 0) > 0 || (modelData.tables ?? 0) > 0
		readonly property int imgW: modelData.width ?? 0
		readonly property int imgH: modelData.height ?? 0

		width: column.width
		spacing: root.lineGap

		ReasoningSkeleton { visible: partCol.kind === "reasoning" }
		TextSkeleton {
			visible: partCol.kind === "text"
			width: parent.width
			linesCount: partCol.linesCount
			hasRichCard: partCol.hasRichCard
		}
		ToolSkeleton {
			visible: partCol.kind === "tool" && partCol.toolName !== "todo"
			width: parent.width
			toolName: partCol.toolName
		}
		TodoSkeleton {
			visible: partCol.kind === "tool" && partCol.toolName === "todo"
			width: parent.width
		}
		ImageSkeleton {
			visible: partCol.kind === "image"
			maxParentWidth: parent.width
			imgW: partCol.imgW
			imgH: partCol.imgH
		}
	}

	component AssistantSkeleton: Column {
		width: column.width
		spacing: root.partGap

		Repeater {
			model: root.skeletonParts
			delegate: PartSkeleton {}
		}
	}

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

		UserSkeleton { visible: root.messageRole === "user" }
		AssistantSkeleton { visible: root.messageRole !== "user" }
	}
}
