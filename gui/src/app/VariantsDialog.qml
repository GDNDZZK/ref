pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Picks the variant (reasoning effort) of the session's model.
SessionDialog {
	id: dialog

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
			selected: modelData === dialog.session.variant
			onActivated: {
				Workspace.setVariant(dialog.sessionId, modelData)
				dialog.close()
			}

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: row.check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: row.modelData
				color: Theme.text
				font.pixelSize: Theme.fontSize
				elide: Text.ElideRight
			}
		}
	}
}
