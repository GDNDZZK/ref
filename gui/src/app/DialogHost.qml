import QtQuick
import RefGui

// Shows the dialog Workspace.dialog names over a scrim. The scrim fades with the dialog; the
// dialog's own item is created when it opens and dropped when the fade is over.
Item {
	id: host

	readonly property bool open: Workspace.dialog !== ""
	// The dialog on screen, also while it fades out.
	property string current: Workspace.dialog

	visible: scrim.opacity > 0

	onOpenChanged: if (open) current = Workspace.dialog
	Connections {
		target: Workspace
		function onDialogChanged() {
			if (Workspace.dialog !== "")
				host.current = Workspace.dialog
		}
	}

	Rectangle {
		id: scrim
		anchors.fill: parent
		color: Theme.scrim
		opacity: host.open ? 1 : 0

		Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
	}

	Loader {
		anchors.fill: parent
		active: host.visible
		sourceComponent: host.componentFor(host.current)
	}

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
