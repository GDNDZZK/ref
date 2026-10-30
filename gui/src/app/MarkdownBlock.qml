// Adapted from an early version of qutie by LightBlueCube
import QtQuick
import RefGui

// Assistant prose, rendered as Markdown while it streams.
Block {
	id: block

	ProseText {
		id: prose
		width: block.columnWidth
		textFormat: TextEdit.MarkdownText
		text: block.content

		StreamCaret {
			visible: block.streaming
			anchorRect: prose.positionToRectangle(prose.length)
		}
	}
}
