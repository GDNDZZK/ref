// Adapted from an early version of LightBlueCube/qutie
import QtQuick
import RefGui

// Assistant prose, rendered as Markdown while it streams.
Block {
	id: block

	ProseText {
		id: prose
		width: block.columnWidth
		streaming: block.streaming
		textFormat: TextEdit.MarkdownText
		text: block.content

		StreamCaret {
			visible: block.streaming
			anchorRect: prose.positionToRectangle(prose.length)
		}
	}
}
