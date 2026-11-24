pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Shows the dialog Workspace.dialog names over a scrim. The scrim fades with the dialog; a
// dialog's own item is created when it opens and dropped when the fade is over. Opening one
// dialog from another keeps the scrim and crossfades the two: each lives in its own slot.
Item {
	id: host

	readonly property bool open: Workspace.dialog !== ""
	property var front: slotA // the slot of the dialog on screen, also while it fades out

	signal quitWindowConfirmed()

	function requestWindowClose() {
		if (Workspace.dialog === DialogId.config && front && front.item) {
			const cfg = front.item as ConfigDialog
			if (cfg)
				return cfg.requestQuitWindow()
		}
		if (Workspace.configEditor.dirty) {
			Workspace.openDialog(DialogId.config, { confirmQuitWindow: true })
			return false
		}
		return true
	}
	visible: open || scrim.opacity > 0

	onVisibleChanged: {
		if (!visible) {
			slotA.retired = false
			slotA.dialogId = ""
			slotB.retired = false
			slotB.dialogId = ""
		}
	}

	Connections {
		target: Workspace
		function onDialogChanged() {
			if (Workspace.dialog === "" || host.front.dialogId === Workspace.dialog)
				return
			const incoming = host.front === slotA ? slotB : slotA
			host.front.retired = true
			incoming.retired = false
			incoming.dialogId = Workspace.dialog
			host.front = incoming
		}
	}

	Rectangle {
		id: scrim
		anchors.fill: parent
		color: Theme.scrim
		opacity: host.open ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	// Absorb wheel and touchpad events that the dialog's own scrollables did not consume.
	MouseArea {
		anchors.fill: parent
		acceptedButtons: Qt.NoButton
		enabled: host.visible
		onWheel: wheel => wheel.accepted = true
	}

	component Slot: Loader {
		id: slot
		property string dialogId: ""
		// Replaced by another dialog: fades out and is dropped.
		property bool retired: false

		anchors.fill: parent
		active: dialogId !== "" && host.visible
		enabled: !retired
		opacity: retired ? 0 : 1
		scale: retired ? Motion.popScale : 1
		sourceComponent: dialogId === "" ? null : host.componentFor(dialogId) // an id without a component is a mistake and logs

		Behavior on opacity {
			enabled: slot.retired
			NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
		}
		Behavior on scale {
			enabled: slot.retired
			NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
		}

		onOpacityChanged: {
			if (retired && opacity === 0)
				dialogId = ""
		}
	}

	Slot { id: slotA }
	Slot { id: slotB }
	function componentFor(id) {
		const components = {
			[DialogId.palette]: paletteDialog,
			[DialogId.settings]: settings,
			[DialogId.config]: configDialog,
			[DialogId.sessions]: sessions,
			[DialogId.models]: models,
			[DialogId.agents]: agents,
			[DialogId.variants]: variants,
			[DialogId.help]: help,
			[DialogId.mcp]: mcp,
			[DialogId.peers]: peers,
			[DialogId.exportSession]: exportDialog,
			[DialogId.cd]: cd,
			[DialogId.newIn]: newIn,
			[DialogId.closeSession]: closeDialog,
			[DialogId.rename]: rename,
			[DialogId.deleteHistory]: deleteHistory,
			[DialogId.stash]: stashDialog,
			[DialogId.backend]: backendDialog
		}
		if (!Fields.has(components, id)) {
			console.error("No dialog for id", id)
			return null
		}
		return components[id]
	}

	Component { id: paletteDialog; CommandPalette {} }
	Component { id: settings; SettingsDialog {} }
	Component { id: configDialog; ConfigDialog { onQuitWindowConfirmed: host.quitWindowConfirmed() } }
	Component { id: sessions; SessionsDialog {} }
	Component { id: models; ModelsDialog {} }
	Component { id: agents; AgentsDialog {} }
	Component { id: variants; VariantsDialog {} }
	Component { id: help; HelpDialog {} }
	Component { id: mcp; McpDialog {} }
	Component { id: peers; PeersDialog {} }
	Component { id: exportDialog; ExportDialog {} }
	Component { id: cd; CdDialog {} }
	Component { id: newIn; NewSessionDialog {} }
	Component { id: closeDialog; CloseDialog {} }
	Component { id: rename; RenameDialog {} }
	Component { id: deleteHistory; DeleteHistoryDialog {} }
	Component { id: stashDialog; StashDialog {} }
	Component { id: backendDialog; BackendDialog {} }
}
