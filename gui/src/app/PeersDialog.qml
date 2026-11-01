import QtQuick
import RefGui

// What the backend says about the other live sessions: peers, this session's name, and the
// messages received and sent.
SessionDialog {
	id: dialog

	readonly property var commands: ({peers: "peers", name: "peers-name", inbox: "peers-inbox", outbox: "peers-outbox"})
	property string tab: Workspace.dialogArgs.tab ?? "peers"
	property string result: ""
	property bool loading: true

	function load() {
		loading = true
		Workspace.peerCommand(sessionId, commands[tab], "", replyWhileOpen((ok, text) => {
			loading = false
			result = text
		}))
	}

	function rename() {
		Workspace.peerCommand(sessionId, "peers-name", nameField.text.trim(), replyWhileOpen((ok, text) => {
			if (ok)
				result = text
			else
				nameField.errorText = text
		}))
	}

	title: qsTr("Peers")
	dialogWidth: Theme.dialogWidthWide
	onTabChanged: load()
	Component.onCompleted: load()

	DialogBody {
		spacing: 14

		Item {
			width: parent.width
			height: 30

			Segmented {
				width: parent.width
				model: [
					{text: qsTr("Peers"), value: "peers"},
					{text: qsTr("Name"), value: "name"},
					{text: qsTr("Inbox"), value: "inbox"},
					{text: qsTr("Outbox"), value: "outbox"}
				]
				value: dialog.tab
				onActivated: value => dialog.tab = value
			}
		}

		Rectangle {
			width: parent.width
			height: Math.min(320, Math.max(200, output.contentHeight + 2 * 14))
			radius: 10
			color: Theme.codeBackground
			border.color: Theme.border
			clip: true

			Flickable {
				anchors { fill: parent; margins: 14 }
				contentWidth: width
				contentHeight: output.contentHeight
				boundsBehavior: Flickable.StopAtBounds

				TextEdit {
					textFormat: TextEdit.PlainText
					id: output
					width: parent.width
					readOnly: true
					selectByMouse: true
					selectionColor: Theme.selection
					selectedTextColor: Theme.text
					wrapMode: TextEdit.Wrap
					text: dialog.loading ? qsTr("Loading…") : dialog.result
					color: dialog.loading ? Theme.textMuted : Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
					opacity: dialog.loading ? 0.6 : 1

					Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}
			}
		}

		Row {
			width: parent.width
			spacing: 10
			visible: dialog.tab === "name"

			DialogField {
				id: nameField
				width: parent.width - setName.width - parent.spacing
				placeholderText: qsTr("Peer name")
				onAccepted: dialog.rename()
			}
			DialogButton {
				id: setName
				text: qsTr("Set name")
				kind: "primary"
				onClicked: dialog.rename()
			}
		}
	}
}
