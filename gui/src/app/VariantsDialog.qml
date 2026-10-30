import QtQuick
import RefGui

// Picks the variant (reasoning effort) of the session's model.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	readonly property var session: Workspace.info(sessionId)
	readonly property var variants: Workspace.config.variantsOf(session.provider, session.modelId)

	title: qsTr("Model variant")
	onAccepted: list.activateCurrent()

	PickList {
		id: list
		width: parent.width
		searchable: false
		emptyText: qsTr("This model has no variants")
		model: dialog.variants
		delegate: PickRow {
			id: row
			required property string modelData
			onActivated: {
				Workspace.setVariant(dialog.sessionId, modelData)
				dialog.close()
			}

			Text {
				anchors { left: parent.left; leftMargin: 22; right: check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: row.modelData
				color: Theme.text
				font.pixelSize: Theme.fontSize
				elide: Text.ElideRight
			}
			Icon {
				id: check
				anchors { right: parent.right; rightMargin: 22; verticalCenter: parent.verticalCenter }
				name: "check"
				size: 16
				color: Theme.accent
				visible: row.modelData === dialog.session.variant
			}
		}
	}
}
