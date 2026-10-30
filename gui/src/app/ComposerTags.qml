import QtQuick
import RefGui

// The tag row under the text: the shell pill, then agent, model and variant, and the hint that
// a second Esc interrupts. The values come from the backend; clicking only asks for a change.
Item {
	id: tags

	required property ComposerModel model
	readonly property int chipGap: 4
	readonly property int hintGap: 12

	signal agentClicked()
	signal modelClicked()
	signal variantClicked()

	implicitHeight: 28

	Row {
		id: row
		spacing: tags.chipGap
		height: parent.height

		ComposerChip {
			anchors.verticalCenter: parent.verticalCenter
			iconName: "terminal"
			iconColor: Theme.shellBorder
			labelColor: Theme.shellBorder
			label: tags.model.shellMode ? qsTr("Shell") : ""
			clickable: false
		}

		ComposerChip {
			anchors.verticalCenter: parent.verticalCenter
			iconName: "agent"
			iconColor: Theme.agentColor(tags.model.agentColor, tags.model.agentIndex)
			label: tags.model.agentName
			tip: qsTr("Agent (Tab)")
			onClicked: tags.agentClicked()
		}

		ComposerChip {
			anchors.verticalCenter: parent.verticalCenter
			iconName: "model"
			label: tags.model.agentName === "" ? "" : tags.model.modelName
			tip: tags.model.modelDescription === "" ? qsTr("Model") : qsTr("Model: %1").arg(tags.model.modelDescription)
			onClicked: tags.modelClicked()
		}

		ComposerChip {
			anchors.verticalCenter: parent.verticalCenter
			iconName: "variant"
			label: tags.model.variantAvailable ? (tags.model.variant === "" ? qsTr("default") : tags.model.variant) : ""
			tip: qsTr("Variant")
			onClicked: tags.variantClicked()
		}
	}

	Text {
		textFormat: Text.PlainText
		anchors { left: row.right; leftMargin: tags.hintGap; verticalCenter: parent.verticalCenter }
		width: Math.max(0, tags.width - row.width - tags.hintGap)
		text: qsTr("Press Esc again to interrupt")
		color: Theme.text
		font.pixelSize: Theme.smallFontSize
		elide: Text.ElideRight
		opacity: tags.model.interruptArmed ? 1 : 0
		visible: opacity > 0

		Behavior on opacity { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
	}
}
