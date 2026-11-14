import QtQuick
import RefGui

// What the backend says about the other live sessions: peers, this session's name, and the
// messages received and sent.
SessionDialog {
	id: dialog

	// The tabs: what the Segmented shows, and the backend command behind each.
	readonly property var tabs: [
		{text: qsTr("Peers"), value: "peers", command: "peers"},
		{text: qsTr("Name"), value: "name", command: "peers-name"},
		{text: qsTr("Inbox"), value: "inbox", command: "peers-inbox"},
		{text: qsTr("Outbox"), value: "outbox", command: "peers-outbox"}
	]
	readonly property int outputMinHeight: 200
	readonly property int outputMaxHeight: 320
	property string tab: Workspace.dialogArgs.tab ?? tabs[0].value
	property string result: ""
	property bool loading: true
	property bool failed: false // the result is the backend's reason the command failed

	function commandOf(tab) {
		return tabs.find(entry => entry.value === tab).command
	}

	function load() {
		loading = true
		Workspace.peerCommand(sessionId, commandOf(tab), "", replyWhileOpen((ok, text) => {
			loading = false
			failed = !ok
			result = text
		}))
	}

	function rename() {
		Workspace.peerCommand(sessionId, commandOf("name"), nameField.text.trim(), replyWhileOpen((ok, text) => {
			if (ok) {
				failed = false
				result = text
			} else {
				nameField.errorText = text
			}
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
				model: dialog.tabs
				value: dialog.tab
				onActivated: value => dialog.tab = value
			}
		}

		Rectangle {
			width: parent.width
			height: Math.min(dialog.outputMaxHeight, Math.max(dialog.outputMinHeight, output.contentHeight + 2 * 14))
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
					color: dialog.loading ? Theme.textMuted : dialog.failed ? Theme.dangerText : Theme.text
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
				kind: DialogButton.Primary
				onClicked: dialog.rename()
			}
		}
	}
}
