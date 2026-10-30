import QtQuick
import RefGui

// A shell command: the command, how it ended and its output. While it runs, the last lines
// of the output so far.
ToolBodyBase {
	id: body

	readonly property var meta: call.meta
	readonly property string command: typeof call.inputObject.command === "string" ? call.inputObject.command : ""
	readonly property bool running: call.toolState === "running"
	readonly property int liveTailLines: 20
	readonly property var liveLines: call.liveOutput === "" ? [] : call.liveOutput.split("\n")
	readonly property int earlierLines: Math.max(0, liveLines.length - liveTailLines)
	readonly property bool hasExit: meta.exit_code !== undefined && meta.exit_code !== null

	CommandBox {
		visible: body.command !== ""
		width: body.contentWidth
		command: body.command
		copyable: true
	}

	ToolSection {
		visible: body.running && body.liveLines.length > 0
		width: body.contentWidth
		label: qsTr("Output")

		Column {
			width: parent.width
			spacing: 4

			Text {
				textFormat: Text.PlainText
				visible: body.earlierLines > 0
				text: qsTr("%n earlier line(s)", "", body.earlierLines)
				color: Theme.textMuted
				font.pixelSize: 11
			}
			OutputText {
				text: body.liveLines.slice(body.earlierLines).join("\n")
				copyable: false
			}
		}
	}

	ToolSection {
		visible: !body.running && (body.call.output !== "" || body.hasExit)
		width: body.contentWidth
		label: qsTr("Output")

		Column {
			width: parent.width
			spacing: 6

			Row {
				spacing: 6
				visible: body.hasExit || body.meta.timed_out === true || body.meta.cancelled === true || body.meta.truncated === true

				ToolChip {
					visible: body.hasExit
					text: qsTr("exit %1").arg(body.meta.exit_code)
					tint: body.meta.exit_code === 0 ? Theme.textMuted : Theme.danger
				}
				ToolChip {
					visible: body.meta.timed_out === true
					text: qsTr("timed out")
					tint: Theme.warning
				}
				ToolChip {
					visible: body.meta.cancelled === true
					text: qsTr("cancelled")
					tint: Theme.warning
				}
				ToolChip {
					visible: body.meta.truncated === true
					text: qsTr("truncated")
				}
			}
			OutputText {
				visible: body.call.output !== ""
				text: body.call.output
				textColor: body.call.failed || body.call.toolState === "error" ? Theme.danger : Theme.textMuted
				showAll: body.call.showAll
				onShowAllRequested: body.showAllRequested()
			}
		}
	}
}
