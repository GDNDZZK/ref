import QtQuick
import QtQuick.Controls.Basic
import RefGui

// Rows of monospace text with an optional line number gutter and sign column, each row
// on its own background: diffs, file contents and search results. Lines are not wrapped;
// a wider block scrolls sideways. Over `previewLines` rows are cut until asked for.
//
// A row is {text, kind} with optional `number` and `sign`; the kinds and what they look like
// are in `styles`.
Item {
	id: view

	required property var rows
	property bool showAll: false
	property string copyText: ""

	signal showAllRequested()

	readonly property int previewLines: 200
	readonly property int rowHeight: 18
	readonly property int gutterGap: 12
	readonly property int signWidth: 16
	readonly property int sidePadding: 8
	readonly property real gutterOpacity: 0.7
	readonly property int scrollBarRoom: 8 // under the last row, while the block scrolls sideways
	readonly property var styles: ({
		"line": { background: "transparent", color: Theme.text },
		"match": { background: Theme.matchBackground, color: Theme.text },
		"context": { background: "transparent", color: Theme.textMuted },
		"gap": { background: "transparent", color: Theme.textMuted },
		"added": { background: Theme.diffAddedBackground, color: Theme.diffAdded },
		"removed": { background: Theme.diffRemovedBackground, color: Theme.diffRemoved },
		"hunk": { background: Theme.surface, color: Theme.info },
		"file": { background: "transparent", color: Theme.textMuted, bold: true }
	})

	readonly property int hiddenRows: showAll ? 0 : Math.max(0, rows.length - previewLines)
	readonly property var shownRows: hiddenRows > 0 ? rows.slice(0, previewLines) : rows
	readonly property int numberChars: shownRows.reduce((most, row) => Math.max(most, row.number ? row.number.length : 0), 0)
	readonly property int textChars: shownRows.reduce((most, row) => Math.max(most, row.text.length), 0)
	readonly property bool hasSigns: shownRows.some(row => row.sign)
	readonly property real gutterWidth: numberChars > 0 ? numberChars * metrics.charWidth + gutterGap : 0
	readonly property real textX: sidePadding + gutterWidth + (hasSigns ? signWidth : 0)
	readonly property real fullWidth: textX + textChars * metrics.charWidth + sidePadding

	width: parent.width
	height: scroller.height + (hiddenRows > 0 ? more.height : 0)

	FontMetrics {
		id: metrics
		font.family: Theme.monoFamily
		font.pixelSize: Theme.smallFontSize
		readonly property real charWidth: advanceWidth("0")
	}

	Flickable {
		id: scroller
		width: parent.width
		height: view.shownRows.length * view.rowHeight + (contentWidth > width ? scrollBarRoom : 0)
		contentWidth: Math.max(width, view.fullWidth)
		contentHeight: height
		flickableDirection: Flickable.HorizontalFlick
		boundsBehavior: Flickable.StopAtBounds
		clip: true

		ScrollBar.horizontal: ScrollBar {
			id: sideBar
			policy: ScrollBar.AlwaysOn
			visible: size < 1
			contentItem: Rectangle {
				implicitHeight: 6
				radius: height / 2
				color: sideBar.pressed ? Theme.scrollHandlePressed : Theme.scrollHandle
			}
		}

		Column {
			width: scroller.contentWidth

			Repeater {
				model: view.shownRows

				delegate: Rectangle {
					id: row

					required property var modelData
					readonly property var style: view.styles[modelData.kind] ?? view.styles["line"]

					width: scroller.contentWidth
					height: view.rowHeight
					color: style.background

					Text {
						textFormat: Text.PlainText
						visible: view.gutterWidth > 0
						x: view.sidePadding
						width: view.gutterWidth - view.gutterGap
						anchors.verticalCenter: parent.verticalCenter
						horizontalAlignment: Text.AlignRight
						text: row.modelData.number ?? ""
						color: Theme.textMuted
						opacity: view.gutterOpacity
						font.family: Theme.monoFamily
						font.pixelSize: 11
					}
					Text {
						textFormat: Text.PlainText
						visible: view.hasSigns
						x: view.sidePadding + view.gutterWidth
						anchors.verticalCenter: parent.verticalCenter
						text: row.modelData.sign ?? ""
						color: row.style.color
						font.family: Theme.monoFamily
						font.pixelSize: Theme.smallFontSize
					}
					Text {
						textFormat: Text.PlainText
						x: row.modelData.kind === "file" || row.modelData.kind === "hunk" ? view.sidePadding + view.gutterWidth : view.textX // headers start where the sign column does
						anchors.verticalCenter: parent.verticalCenter
						text: row.modelData.text
						color: row.style.color
						font.family: Theme.monoFamily
						font.pixelSize: Theme.smallFontSize
						font.bold: row.style.bold ?? false
					}
				}
			}
		}
	}

	TextButton {
		id: more
		y: scroller.height
		visible: view.hiddenRows > 0
		text: qsTr("Show %n more line(s)", "", view.hiddenRows)
		onClicked: view.showAllRequested()
	}

	HoverHandler { id: hover }

	CopyButton {
		anchors { top: parent.top; right: parent.right; margins: 4 }
		visible: view.copyText !== ""
		text: view.copyText
		opacity: hover.hovered ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
}
