import QtQuick
import RefGui

// A shell command: the command, how it ended and its output. While it runs, the last lines
// of the output so far.
ToolBodyBase {
	id: body

	readonly property var meta: call.meta
	readonly property string command: Fields.stringOf(call.inputObject, "command")
	readonly property bool running: call.toolState === ToolState.running
	readonly property int liveTailLines: 20
	readonly property var liveLines: call.liveOutput === "" ? [] : call.liveOutput.split("\n")
	readonly property int earlierLines: Math.max(0, liveLines.length - liveTailLines)
	readonly property bool hasExit: meta.exit_code !== undefined && meta.exit_code !== null
	readonly property bool timedOut: meta.timed_out === true
	readonly property bool cancelled: meta.cancelled === true
	readonly property bool truncated: meta.truncated === true
	readonly property bool hasFlags: hasExit || timedOut || cancelled || truncated

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
				font.pixelSize: Theme.microFontSize
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
				visible: body.hasFlags

				ToolChip {
					visible: body.hasExit
					text: qsTr("exit %1").arg(body.meta.exit_code)
					tint: body.meta.exit_code === 0 ? Theme.textMuted : Theme.danger
					textTint: body.meta.exit_code === 0 ? Theme.textMuted : Theme.dangerText
				}
				ToolChip {
					visible: body.timedOut
					text: qsTr("timed out")
					tint: Theme.warning
					textTint: Theme.warningText
				}
				ToolChip {
					visible: body.cancelled
					text: qsTr("cancelled")
					tint: Theme.warning
					textTint: Theme.warningText
				}
				TruncatedChip {
					meta: body.meta
				}
			}
			OutputText {
				visible: body.call.output !== ""
				text: body.call.output
				textColor: body.call.failed || body.call.toolState === ToolState.error ? Theme.dangerText : Theme.textMuted
				showAll: body.call.showAll
				onShowAllRequested: body.showAllRequested()
			}
		}
	}
}
