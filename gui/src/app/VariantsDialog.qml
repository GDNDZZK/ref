pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Picks the variant (reasoning effort) of the session's model.
SessionDialog {
	id: dialog

	readonly property var rawVariants: {
		void Workspace.config.models
		return Workspace.config.variantsOf(session.provider, session.modelId)
	}
	readonly property var options: {
		if (rawVariants.length === 0 && session.variant === "")
			return []
		return [{value: "", label: qsTr("default")}, ...rawVariants.map(name => ({value: name, label: name}))]
	}
	readonly property int selectedIndex: options.findIndex(option => option.value === session.variant)

	title: qsTr("Model variant")
	onAccepted: list.activateCurrent()

	PickList {
		id: list
		width: parent.width
		focus: true
		searchable: false
		initialIndex: Math.max(0, dialog.selectedIndex)
		emptyText: qsTr("This model has no variants")
		model: dialog.options
		delegate: PickRow {
			id: row
			required property var modelData
			selected: modelData.value === dialog.session.variant
			onActivated: {
				if (modelData.value === "")
					Workspace.clearVariant(dialog.sessionId)
				else
					Workspace.setVariant(dialog.sessionId, modelData.value)
				dialog.close()
			}

			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: Theme.dialogPadding; right: row.check.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: row.modelData.label
				color: Theme.text
				font.pixelSize: Theme.fontSize
				elide: Text.ElideRight
			}
		}
	}
}
