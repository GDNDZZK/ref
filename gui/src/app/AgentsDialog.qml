pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Picks the agent of one session.
SessionDialog {
	id: dialog

	readonly property string needle: list.text.trim().toLowerCase()
	readonly property var shown: {
		const agents = Workspace.config.agents.map((agent, index) => Object.assign({position: index}, agent))
		return agents.filter(agent => needle === "" || (agent.name + " " + agent.description).toLowerCase().includes(needle))
	}

	title: qsTr("Switch agent")
	onAccepted: list.activateCurrent()

	PickList {
		id: list
		width: parent.width
		placeholderText: qsTr("Search agents")
		model: dialog.shown
		delegate: PickRow {
			id: row
			required property var modelData
			selected: modelData.name === dialog.session.agent
			onActivated: {
				Workspace.setAgent(dialog.sessionId, modelData.name)
				dialog.close()
			}

			Rectangle {
				x: Theme.dialogPadding
				anchors.verticalCenter: parent.verticalCenter
				width: Theme.statusDotSize
				height: Theme.statusDotSize
				radius: width / 2
				color: Theme.agentColor(row.modelData.color, row.modelData.position)
			}
			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding + 20; right: row.check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: row.modelData.name
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}
				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: row.modelData.description
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}
		}
	}
}
