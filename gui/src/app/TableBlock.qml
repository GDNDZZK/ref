pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import RefGui

// A GFM pipe table rendered as a rounded card with independent selectable ProseText cells.
Block {
	id: block

	required property var tableHeaders
	required property var tableAlignments
	required property var tableRows

	readonly property int cellPadH: 12
	readonly property int cellPadV: 7
	readonly property int minColWidth: 48
	readonly property int minReadableColWidth: 140
	readonly property int horizontalBarReserve: 8
	readonly property real availWidth: Math.max(0, block.columnWidth - 2)

	FontMetrics {
		id: regularMetrics
		font.pixelSize: Theme.fontSize
	}

	FontMetrics {
		id: boldMetrics
		font.pixelSize: Theme.fontSize
		font.bold: true
	}

	function plainCellEstimate(text) {
		return String(text ?? "")
			.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
			.replace(/[*_`~]/g, "")
	}

	function measureWidth(text, isBold) {
		const fm = isBold ? boldMetrics : regularMetrics
		const lines = plainCellEstimate(text).split("\n")
		let maxLine = 0
		for (let i = 0; i < lines.length; ++i)
			maxLine = Math.max(maxLine, Math.ceil(fm.advanceWidth(lines[i])))
		return maxLine + 2 * cellPadH
	}

	readonly property var columnWidths: {
		const headers = block.tableHeaders ?? []
		const rows = block.tableRows ?? []
		const count = headers.length
		if (count === 0)
			return []

		const naturals = []
		const mins = []
		let sumNatural = 0
		let sumMin = 0

		for (let c = 0; c < count; ++c) {
			let maxW = Math.max(minColWidth, measureWidth(headers[c], true))
			for (let r = 0; r < rows.length; ++r) {
				const row = rows[r]
				if (row && c < row.length)
					maxW = Math.max(maxW, measureWidth(row[c], false))
			}
			const minW = Math.min(maxW, minReadableColWidth)
			naturals.push(maxW)
			mins.push(minW)
			sumNatural += maxW
			sumMin += minW
		}

		const avail = block.availWidth
		if (avail <= 0 || sumMin >= avail)
			return mins

		const widths = []
		if (sumNatural <= avail) {
			const extra = avail - sumNatural
			let assigned = 0
			for (let c = 0; c < count; ++c) {
				const w = c === count - 1
					? Math.max(minColWidth, avail - assigned)
					: Math.round(naturals[c] + extra * (naturals[c] / sumNatural))
				widths.push(w)
				assigned += w
			}
			return widths
		}

		const budget = avail - sumMin
		const demand = sumNatural - sumMin
		let assigned = 0
		for (let c = 0; c < count; ++c) {
			const w = c === count - 1
				? Math.max(mins[c], avail - assigned)
				: Math.round(mins[c] + (demand > 0 ? budget * ((naturals[c] - mins[c]) / demand) : 0))
			widths.push(w)
			assigned += w
		}
		return widths
	}

	readonly property real totalTableWidth: {
		let total = 0
		for (let i = 0; i < columnWidths.length; ++i)
			total += columnWidths[i]
		return total
	}

	readonly property bool overflowsHorizontally: totalTableWidth > availWidth

	function alignmentFor(colIndex) {
		const align = block.tableAlignments && colIndex < block.tableAlignments.length ? block.tableAlignments[colIndex] : "left"
		if (align === "center")
			return TextEdit.AlignHCenter
		if (align === "right")
			return TextEdit.AlignRight
		return TextEdit.AlignLeft
	}

	Rectangle {
		id: card

		width: block.columnWidth
		height: tableColumn.height + 2 + (block.overflowsHorizontally ? block.horizontalBarReserve : 0)
		radius: Theme.radius
		color: Theme.background
		border.width: 1
		border.color: Theme.border
		clip: true

		Flickable {
			id: scroller

			x: 1
			y: 1
			width: parent.width - 2
			height: parent.height - 2
			contentWidth: tableColumn.width
			contentHeight: tableColumn.height
			clip: true
			boundsBehavior: Flickable.StopAtBounds
			acceptedButtons: Qt.NoButton
			interactive: contentWidth > width

			ScrollBar.horizontal: ThemedScrollBar {
				orientation: Qt.Horizontal
			}

			Column {
				id: tableColumn

				width: Math.max(scroller.width, block.totalTableWidth)

				Rectangle {
					id: headerRow

					width: tableColumn.width
					height: headerLayout.implicitHeight + 1
					color: Theme.surface
					topLeftRadius: Math.max(0, card.radius - card.border.width)
					topRightRadius: topLeftRadius

					RowLayout {
						id: headerLayout

						width: parent.width
						spacing: 0

						Repeater {
							model: block.tableHeaders ?? []

							Item {
								id: headerCell

								required property int index
								required property string modelData

								readonly property real colW: headerCell.index < block.columnWidths.length ? block.columnWidths[headerCell.index] : block.minColWidth

								Layout.preferredWidth: colW
								Layout.minimumWidth: colW
								Layout.maximumWidth: colW
								Layout.fillHeight: true
								implicitHeight: headerText.implicitHeight + 2 * block.cellPadV

								ProseText {
									id: headerText

									x: block.cellPadH
									y: block.cellPadV
									width: Math.max(0, parent.width - 2 * block.cellPadH)
									textFormat: TextEdit.MarkdownText
									text: headerCell.modelData
									streaming: block.streaming
									font.bold: true
									horizontalAlignment: block.alignmentFor(headerCell.index)
								}

								Rectangle {
									visible: headerCell.index < (block.tableHeaders ? block.tableHeaders.length - 1 : 0)
									anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
									width: 1
									color: Theme.border
								}
							}
						}
					}

					Rectangle {
						anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
						height: 1
						color: Theme.border
					}
				}

				Repeater {
					model: block.tableRows ?? []

					Rectangle {
						id: bodyRow

						required property int index
						required property var modelData

						readonly property int rowIndex: bodyRow.index
						readonly property bool isLastRow: bodyRow.rowIndex === ((block.tableRows ? block.tableRows.length : 0) - 1)

						width: tableColumn.width
						height: bodyLayout.implicitHeight + (bodyRow.isLastRow ? 0 : 1)
						color: bodyRow.rowIndex % 2 === 1 ? Qt.alpha(Theme.surface, 0.4) : Theme.background
						bottomLeftRadius: bodyRow.isLastRow && !block.overflowsHorizontally ? Math.max(0, card.radius - card.border.width) : 0
						bottomRightRadius: bottomLeftRadius

						RowLayout {
							id: bodyLayout

							width: parent.width
							spacing: 0

							Repeater {
								model: bodyRow.modelData ?? []

								Item {
									id: bodyCell

									required property int index
									required property string modelData

									readonly property real colW: bodyCell.index < block.columnWidths.length ? block.columnWidths[bodyCell.index] : block.minColWidth

									Layout.preferredWidth: colW
									Layout.minimumWidth: colW
									Layout.maximumWidth: colW
									Layout.fillHeight: true
									implicitHeight: bodyText.implicitHeight + 2 * block.cellPadV

									ProseText {
										id: bodyText

										x: block.cellPadH
										y: block.cellPadV
										width: Math.max(0, parent.width - 2 * block.cellPadH)
										textFormat: TextEdit.MarkdownText
										text: bodyCell.modelData
										streaming: block.streaming
										horizontalAlignment: block.alignmentFor(bodyCell.index)
									}

									Rectangle {
										visible: bodyCell.index < (bodyRow.modelData ? bodyRow.modelData.length - 1 : 0)
										anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
										width: 1
										color: Theme.border
									}
								}
							}
						}

						Rectangle {
							visible: !bodyRow.isLastRow
							anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
							height: 1
							color: Theme.border
						}
					}
				}
			}
		}

		HoverCopyButton {
			anchors { top: parent.top; right: parent.right; topMargin: 4; rightMargin: 4 }
			hoverTarget: card
			visible: block.content !== ""
			text: block.content
		}
	}
}
