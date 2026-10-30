import QtQuick
import RefGui

// Picks the model of one session, grouped by provider.
ModalDialog {
	id: dialog

	readonly property string sessionId: Workspace.dialogArgs.sessionId ?? Workspace.currentId
	readonly property var session: Workspace.info(sessionId)
	readonly property string needle: list.text.trim().toLowerCase()

	function details(model) {
		const parts = []
		if (model.context > 0)
			parts.push(qsTr("%1 context").arg(Format.tokens(model.context)))
		if (model.reasoning)
			parts.push(qsTr("reasoning"))
		if (model.images)
			parts.push(qsTr("images"))
		return parts.join(" · ")
	}

	// A ListModel, because only a model with roles can be sectioned.
	function rebuild() {
		items.clear()
		for (const model of Workspace.config.models) {
			if (needle !== "" && !(model.name + " " + model.id + " " + model.providerName).toLowerCase().includes(needle))
				continue
			items.append({
				provider: model.provider,
				providerName: model.providerName,
				modelId: model.id,
				name: model.name,
				summary: details(model),
				variants: model.variants.join(",")
			})
		}
	}

	title: qsTr("Switch model")
	onAccepted: list.activateCurrent()
	onNeedleChanged: rebuild()
	Component.onCompleted: rebuild()

	ListModel { id: items }

	PickList {
		id: list
		width: parent.width
		placeholderText: qsTr("Search models")
		sectionProperty: "providerName"
		model: items
		delegate: PickRow {
			id: row
			required property string provider
			required property string modelId
			required property string name
			required property string summary
			required property string variants
			onActivated: {
				// The variant stays when the new model has it.
				const keep = variants.split(",").includes(dialog.session.variant) ? dialog.session.variant : ""
				Workspace.setModel(dialog.sessionId, provider, modelId, keep)
				dialog.close()
			}

			Column {
				anchors { left: parent.left; leftMargin: 22; right: check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				spacing: 2

				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: row.name
					color: Theme.text
					font.pixelSize: Theme.fontSize
					elide: Text.ElideRight
				}
				Text {
					textFormat: Text.PlainText
					width: parent.width
					text: row.summary
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
				visible: row.provider === dialog.session.provider && row.modelId === dialog.session.modelId
			}
		}
	}
}
