pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Picks the agent of one session.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	readonly property var session: Workspace.info(sessionId)
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
			onActivated: {
				Workspace.setAgent(dialog.sessionId, modelData.name)
				dialog.close()
			}

			Rectangle {
				x: 22
				anchors.verticalCenter: parent.verticalCenter
				width: 8
				height: 8
				radius: 4
				color: Theme.agentColor(row.modelData.color, row.modelData.position)
			}
			Column {
				anchors { left: parent.left; leftMargin: 42; right: check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
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
			Icon {
				id: check
				anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
				name: "check"
				size: 16
				color: Theme.accent
				visible: row.modelData.name === dialog.session.agent
			}
		}
	}
}
