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

	visible: scrim.opacity > 0

	onVisibleChanged: {
		if (!visible) {
			slotA.dialogId = ""
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

	component Slot: Loader {
		property string dialogId: ""
		// Replaced by another dialog: fades out and is dropped.
		property bool retired: false

		anchors.fill: parent
		active: dialogId !== "" && host.visible
		enabled: !retired
		opacity: retired ? 0 : 1
		sourceComponent: host.componentFor(dialogId)

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		onOpacityChanged: {
			if (retired && opacity === 0)
				dialogId = ""
		}
	}

	Slot { id: slotA }
	Slot { id: slotB }

	function componentFor(id) {
		return {
			"palette": paletteDialog,
			"settings": settings,
			"sessions": sessions,
			"models": models,
			"agents": agents,
			"variants": variants,
			"help": help,
			"mcp": mcp,
			"peers": peers,
			"export": exportDialog,
			"cd": cd,
			"new-in": newIn,
			"close": closeDialog,
			"rename": rename,
			"delete-history": deleteHistory
		}[id] ?? null
	}

	Component { id: paletteDialog; CommandPalette {} }
	Component { id: settings; SettingsDialog {} }
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
}
