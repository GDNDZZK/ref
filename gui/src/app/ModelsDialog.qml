pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Picks the model of one session, grouped by provider.
SessionDialog {
	id: dialog

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

	function matchesNeedle(model) {
		return needle === "" || (model.name + " " + model.id + " " + model.providerName).toLowerCase().includes(needle)
	}

	// A ListModel, because only a model with roles can be sectioned.
	function rebuild() {
		items.clear()
		for (const model of Workspace.config.models) {
			if (!matchesNeedle(model))
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
			selected: provider === dialog.session.provider && modelId === dialog.session.modelId
			onActivated: {
				// The variant stays when the new model has it.
				const keep = variants.split(",").includes(dialog.session.variant) ? dialog.session.variant : ""
				Workspace.setModel(dialog.sessionId, provider, modelId, keep)
				dialog.close()
			}

			Column {
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: row.check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
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

		}
	}
}
