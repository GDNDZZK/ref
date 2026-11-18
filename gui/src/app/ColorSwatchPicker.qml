pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Agent color picker: theme-adaptive named swatches plus custom `#rrggbb` / `#rgb` hex input.
Column {
	id: picker

	property string value: ""
	property string draftText: ""

	signal valueModified(string newValue)

	readonly property var swatches: [
		{ name: "", label: qsTr("Automatic"), aliasNames: [] },
		{ name: "orange", label: qsTr("Orange"), aliasNames: ["primary"] },
		{ name: "blue", label: qsTr("Blue"), aliasNames: ["secondary"] },
		{ name: "purple", label: qsTr("Purple"), aliasNames: ["accent"] },
		{ name: "green", label: qsTr("Green"), aliasNames: ["success"] },
		{ name: "yellow", label: qsTr("Yellow"), aliasNames: ["warning"] },
		{ name: "cyan", label: qsTr("Cyan"), aliasNames: ["info"] },
		{ name: "red", label: qsTr("Red"), aliasNames: ["error"] }
	]

	readonly property string normalizedValue: value.trim().toLowerCase()
	readonly property bool isValidSpec: isValidColorSpec(draftText)
	readonly property bool hasValidationError: !isValidSpec
	readonly property bool hasUncommittedEdit: draftText.trim() !== value.trim()

	function isValidColorSpec(raw) {
		const norm = String(raw ?? "").trim().toLowerCase()
		if (norm === "")
			return true
		if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/.test(norm))
			return true
		return swatches.some(s => s.name === norm || s.aliasNames.includes(norm))
	}

	function matchesSwatch(swatch) {
		if (swatch.name === "")
			return normalizedValue === ""
		return normalizedValue === swatch.name || swatch.aliasNames.includes(normalizedValue)
	}

	function selectColor(spec) {
		const trimmed = spec.trim()
		draftText = trimmed
		hexInput.text = trimmed
		if (trimmed !== value) {
			value = trimmed
			valueModified(trimmed)
		}
	}

	function resetState() {
		draftText = value
		if (hexInput.text !== value)
			hexInput.text = value
	}

	spacing: 6
	opacity: enabled ? 1 : Theme.disabledOpacity

	onValueChanged: {
		if (!hexInput.activeFocus)
			resetState()
	}
	Component.onCompleted: resetState()

	Row {
		spacing: 8

		Row {
			anchors.verticalCenter: parent.verticalCenter
			spacing: 6

			Repeater {
				model: picker.swatches

				delegate: Item {
					id: swatchItem
					required property var modelData
					readonly property bool selected: picker.matchesSwatch(modelData)
					readonly property bool isAuto: modelData.name === ""
					readonly property color fill: isAuto ? "transparent" : Theme.agentColor(modelData.name, 0)

					width: 24
					height: 24

					Rectangle {
						anchors.fill: parent
						radius: width / 2
						color: "transparent"
						border.width: swatchItem.selected ? 2 : 0
						border.color: swatchItem.isAuto ? Theme.accentText : swatchItem.fill
					}

					Rectangle {
						anchors.centerIn: parent
						width: swatchItem.selected ? 16 : 18
						height: width
						radius: width / 2
						color: swatchItem.isAuto ? Theme.surface : swatchItem.fill
						border.width: swatchItem.isAuto ? 1 : 0
						border.color: Theme.border
						scale: swatchTap.pressed ? Motion.pressedScaleButton : 1

						Behavior on width { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
						Behavior on scale { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }

						Text {
							textFormat: Text.PlainText
							visible: swatchItem.isAuto
							anchors.centerIn: parent
							text: "A"
							color: Theme.textMuted
							font { pixelSize: Theme.microFontSize; bold: true }
						}
					}

					HoverHandler {
						id: swatchHover
						enabled: picker.enabled
						cursorShape: Qt.PointingHandCursor
					}
					TapHandler {
						id: swatchTap
						enabled: picker.enabled
						onTapped: picker.selectColor(swatchItem.modelData.name)
					}

					PlainToolTip {
						visible: swatchHover.hovered
						text: swatchItem.modelData.label
					}

					Accessible.role: Accessible.RadioButton
					Accessible.name: modelData.label
					Accessible.checked: selected
					Accessible.onPressAction: picker.selectColor(modelData.name)
				}
			}
		}

		Rectangle {
			width: 124
			height: 32
			anchors.verticalCenter: parent.verticalCenter
			radius: Theme.radiusSmall + 2
			color: Theme.background
			border.color: !picker.isValidSpec
				? Theme.dangerText
				: (hexInput.activeFocus ? Theme.accentText : Theme.border)

			Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

			Rectangle {
				id: previewDot
				x: 8
				anchors.verticalCenter: parent.verticalCenter
				width: 12
				height: 12
				radius: 6
				color: picker.normalizedValue === ""
					? Theme.textMuted
					: Theme.agentColor(picker.value, -1)
				opacity: picker.normalizedValue === "" ? 0.45 : 1
			}

			TextInput {
				id: hexInput
				anchors { left: previewDot.right; leftMargin: 8; right: parent.right; rightMargin: 8; top: parent.top; bottom: parent.bottom }
				verticalAlignment: TextInput.AlignVCenter
				color: Theme.text
				selectionColor: Theme.selection
				selectedTextColor: Theme.text
				font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
				clip: true
				Accessible.name: qsTr("Hex color")
				onTextEdited: {
					picker.draftText = text
					const trimmed = text.trim()
					if (picker.isValidColorSpec(trimmed) && trimmed !== picker.value) {
						picker.value = trimmed
						picker.valueModified(trimmed)
					}
				}
				onEditingFinished: {
					picker.draftText = text
					const trimmed = text.trim()
					if (picker.isValidColorSpec(trimmed)) {
						picker.draftText = trimmed
						if (text !== trimmed)
							text = trimmed
						if (trimmed !== picker.value) {
							picker.value = trimmed
							picker.valueModified(trimmed)
						}
					}
				}

				PlaceholderText {
					anchors.fill: parent
					textInput: hexInput
					text: "#rrggbb"
					font.pixelSize: Theme.smallFontSize
					elide: Text.ElideRight
				}
			}
		}
	}

	Text {
		textFormat: Text.PlainText
		visible: picker.hasValidationError
		text: qsTr("Enter a swatch color name or #rgb / #rrggbb hex code.")
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}
}
