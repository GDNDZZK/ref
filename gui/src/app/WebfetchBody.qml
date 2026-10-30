import QtQuick
import RefGui

// A fetched page: the address, what came back and the text of it.
ToolBodyBase {
	id: body

	readonly property var meta: call.meta
	readonly property string url: typeof call.inputObject.url === "string" ? call.inputObject.url : ""

	Row {
		visible: body.url !== ""
		width: body.contentWidth
		spacing: 8

		Text {
			textFormat: Text.PlainText
			width: parent.width - parent.spacing - 24
			anchors.verticalCenter: parent.verticalCenter
			text: body.url
			elide: Text.ElideMiddle
			color: Theme.info
			font.family: Theme.monoFamily
			font.pixelSize: Theme.smallFontSize
		}
		Item {
			width: 24
			height: 24

			Icon {
				anchors.centerIn: parent
				name: "external-link"
				color: openHover.hovered ? Theme.text : Theme.textMuted
			}
			HoverHandler { id: openHover; cursorShape: Qt.PointingHandCursor }
			TapHandler { onTapped: Links.open(body.url) }
			Accessible.role: Accessible.Button
			Accessible.name: qsTr("Open in browser")
		}
	}

	Row {
		visible: body.meta.status !== undefined || body.meta.bytes !== undefined
		spacing: 6

		ToolChip {
			visible: body.meta.status !== undefined
			text: String(body.meta.status)
			tint: body.meta.status >= 400 ? Theme.danger : Theme.textMuted
		}
		ToolChip {
			visible: typeof body.meta.content_type === "string" && body.meta.content_type !== ""
			text: body.meta.content_type ?? ""
		}
		ToolChip {
			visible: body.meta.bytes !== undefined
			text: ToolInfo.sizeText(body.meta.bytes ?? 0)
		}
		ToolChip {
			visible: body.meta.truncated === true
			text: qsTr("truncated")
		}
	}

	OutputSection {
		call: body.call
		width: body.contentWidth
		onShowAllRequested: body.showAllRequested()
	}
}
