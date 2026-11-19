pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import RefGui

// Dropdown selector over `{label, value, description?}` (or plain string) options, with optional
// free-form text editing (`editable: true`) and clear button (`clearable: true`).
FocusScope {
	id: field

	// Array of `{label, value, description?}` objects or strings.
	property var options: []
	property string value: ""
	property string placeholderText: qsTr("Select…")
	property string errorText: ""
	property bool editable: false
	property bool clearable: false
	readonly property bool hasError: errorText !== ""
	readonly property int maxVisibleRows: 7
	readonly property int optionRowHeight: 36
	property bool animateHighlight: false

	signal activated(string newValue)

	readonly property var normalizedOptions: options.map(entry => {
		if (typeof entry === "string")
			return { label: entry, value: entry, description: "" }
		const val = String(entry.value ?? "")
		return {
			label: String(entry.label ?? val),
			value: val,
			description: String(entry.description ?? "")
		}
	})

	readonly property var filteredOptions: {
		if (!editable || !popup.opened)
			return normalizedOptions
		const needle = editor.text.trim().toLowerCase()
		if (needle === "" || needle === value.toLowerCase())
			return normalizedOptions
		return normalizedOptions.filter(opt =>
			(opt.label + " " + opt.value + " " + opt.description).toLowerCase().includes(needle))
	}

	readonly property string displayLabel: {
		for (const opt of normalizedOptions) {
			if (opt.value === value)
				return opt.label
		}
		return value
	}

	function openPopup() {
		if (!enabled || (filteredOptions.length === 0 && !editable))
			return
		animateHighlight = false
		hoverGuard.reset()
		let idx = filteredOptions.findIndex(opt => opt.value === value)
		if (idx < 0 && filteredOptions.length > 0)
			idx = 0
		listView.currentIndex = idx
		popup.open()
		if (idx >= 0)
			listView.positionViewAtIndex(idx, ListView.Contain)
	}

	function moveSelection(delta) {
		if (filteredOptions.length === 0)
			return
		if (!popup.opened) {
			openPopup()
			return
		}
		const base = listView.currentIndex < 0 ? 0 : listView.currentIndex
		const next = (base + delta + filteredOptions.length) % filteredOptions.length
		animateHighlight = true
		listView.currentIndex = next
		listView.positionViewAtIndex(next, ListView.Contain)
	}

	function chooseOption(newValue) {
		popup.close()
		if (editable)
			editor.text = newValue
		if (newValue !== value)
			value = newValue
		activated(newValue)
	}

	function acceptCurrent() {
		if (popup.opened && listView.currentIndex >= 0 && listView.currentIndex < filteredOptions.length) {
			chooseOption(filteredOptions[listView.currentIndex].value)
			return
		}
		if (editable)
			chooseOption(editor.text.trim())
	}

	implicitWidth: 240
	implicitHeight: Theme.fieldHeight + (hasError ? errorLabel.implicitHeight + 6 : 0)
	opacity: enabled ? 1 : Theme.disabledOpacity

	onValueChanged: {
		if (editable && !editor.activeFocus && editor.text !== value)
			editor.text = value
	}
	Component.onCompleted: {
		if (editable)
			editor.text = value
	}

	Keys.onDownPressed: event => {
		moveSelection(1)
		event.accepted = true
	}
	Keys.onUpPressed: event => {
		moveSelection(-1)
		event.accepted = true
	}
	Keys.onSpacePressed: event => {
		if (!editable) {
			if (popup.opened)
				acceptCurrent()
			else
				openPopup()
			event.accepted = true
		}
	}
	Keys.onReturnPressed: event => {
		if (popup.opened || editable) {
			acceptCurrent()
			event.accepted = true
		}
	}
	Keys.onEnterPressed: event => {
		if (popup.opened || editable) {
			acceptCurrent()
			event.accepted = true
		}
	}

	PointerMoveGuard { id: hoverGuard }

	Rectangle {
		id: box
		width: parent.width
		height: Theme.fieldHeight
		radius: Theme.radius
		color: Theme.background
		border.color: field.hasError
			? Theme.dangerText
			: (field.activeFocus || popup.opened ? Theme.accentText : (boxHover.hovered && field.enabled ? Theme.textMuted : Theme.border))

		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		Text {
			id: staticLabel
			textFormat: Text.PlainText
			visible: !field.editable
			anchors {
				left: parent.left
				leftMargin: 12
				right: trailingRow.left
				rightMargin: 8
				verticalCenter: parent.verticalCenter
			}
			text: field.displayLabel !== "" ? field.displayLabel : field.placeholderText
			color: field.displayLabel !== "" ? Theme.text : Theme.textMuted
			font.pixelSize: Theme.secondaryFontSize
			elide: Text.ElideRight
		}

		TextInput {
			id: editor
			visible: field.editable
			anchors {
				left: parent.left
				leftMargin: 12
				right: trailingRow.left
				rightMargin: 8
				top: parent.top
				bottom: parent.bottom
			}
			verticalAlignment: TextInput.AlignVCenter
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font.pixelSize: Theme.secondaryFontSize
			clip: true
			focus: field.editable
			Accessible.name: field.placeholderText
			onTextEdited: {
				if (!popup.opened && field.filteredOptions.length > 0)
					field.openPopup()
				const trimmed = text.trim()
				if (trimmed !== field.value) {
					field.value = trimmed
					field.activated(trimmed)
				}
			}
			onActiveFocusChanged: {
				if (!activeFocus)
					text = field.value
			}

			PlaceholderText {
				anchors.fill: parent
				textInput: editor
				text: field.placeholderText
				font.pixelSize: Theme.secondaryFontSize
				elide: Text.ElideRight
			}
		}

		Row {
			id: trailingRow
			anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
			spacing: 2

			IconButton {
				visible: field.clearable && field.value !== "" && field.enabled
				anchors.verticalCenter: parent.verticalCenter
				buttonSize: 24
				iconName: "close"
				iconSize: 16
				Accessible.name: qsTr("Clear")
				onClicked: field.chooseOption("")
			}

			Item {
				width: 24
				height: 24
				anchors.verticalCenter: parent.verticalCenter

				Chevron {
					anchors.centerIn: parent
					rotation: popup.opened ? -90 : 90
					color: Theme.textMuted

					Behavior on rotation { NumberAnimation { duration: Motion.quick; easing.type: Motion.ease } }
				}

				TapHandler {
					enabled: field.editable && field.enabled
					onTapped: {
						if (popup.opened)
							popup.close()
						else
							field.openPopup()
					}
				}
			}
		}

		HoverHandler {
			id: boxHover
			enabled: field.enabled && !field.editable
			cursorShape: Qt.PointingHandCursor
		}
		TapHandler {
			enabled: field.enabled && !field.editable
			onTapped: {
				field.forceActiveFocus()
				if (popup.opened)
					popup.close()
				else
					field.openPopup()
			}
		}
	}

	Popup {
		id: popup
		objectName: "selectPopup"
		readonly property real uiScale: UiScale.factor

		y: box.height + 4
		width: popupInner.width * uiScale
		height: popupInner.height * uiScale
		padding: 0
		visible: false
		closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
		background: null

		enter: Transition {
			NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.quick; easing.type: Motion.ease }
			NumberAnimation { property: "scale"; from: Motion.popScale; to: 1; duration: Motion.quick; easing.type: Motion.ease }
		}
		exit: Transition {
			NumberAnimation { property: "opacity"; to: 0; duration: Motion.quick; easing.type: Motion.ease }
		}

		contentItem: Item {
			Item {
				id: popupInner
				readonly property int popupPadding: 4
				width: Math.max(box.width, 200)
				height: Math.min(field.filteredOptions.length, field.maxVisibleRows) * field.optionRowHeight + 2 * popupPadding
				transformOrigin: Item.TopLeft
				scale: popup.uiScale

				Rectangle {
					anchors.fill: parent
					radius: Theme.radius
					color: Theme.surface
					border.color: Theme.border
				}

				ListView {
					id: listView
					anchors { fill: parent; margins: popupInner.popupPadding }
					clip: true
			boundsBehavior: Flickable.StopAtBounds
			model: field.filteredOptions
			highlightFollowsCurrentItem: false

			highlight: Rectangle {
				y: listView.currentItem ? listView.currentItem.y : 0
				width: listView.width
				height: field.optionRowHeight
				radius: Theme.radiusSmall
				color: Theme.surfaceHover
				visible: listView.currentItem !== null

				Behavior on y {
					enabled: field.animateHighlight
					NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
				}
			}

			HoverHandler {
				onPointChanged: {
					if (!hovered || !hoverGuard.movedTo(point.scenePosition))
						return
					const idx = listView.indexAt(point.position.x + listView.contentX, point.position.y + listView.contentY)
					if (idx >= 0) {
						field.animateHighlight = true
						listView.currentIndex = idx
					}
				}
			}

			delegate: Item {
				id: optRow
				required property var modelData
				required property int index
				readonly property bool chosen: modelData.value === field.value

				width: listView.width
				height: field.optionRowHeight

				Text {
					id: optLabel
					textFormat: Text.PlainText
					anchors {
						left: parent.left
						leftMargin: 10
						right: optCheck.visible ? optCheck.left : parent.right
						rightMargin: 8
						verticalCenter: parent.verticalCenter
					}
					text: optRow.modelData.description !== ""
						? optRow.modelData.label + " · " + optRow.modelData.description
						: optRow.modelData.label
					color: Theme.text
					font { pixelSize: Theme.secondaryFontSize; bold: optRow.chosen }
					elide: Text.ElideRight
				}

				Icon {
					id: optCheck
					visible: optRow.chosen
					anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
					name: "check"
					size: 16
					color: Theme.accentText
				}

				HoverHandler { cursorShape: Qt.PointingHandCursor }
				TapHandler { onTapped: field.chooseOption(optRow.modelData.value) }
			}

			ScrollBar.vertical: ThemedScrollBar {}
		}
			}
		}
	}

	Text {
		id: errorLabel
		textFormat: Text.PlainText
		visible: field.hasError
		anchors { top: box.bottom; topMargin: 6; left: parent.left; right: parent.right }
		text: field.errorText
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}

	Accessible.role: Accessible.ComboBox
	Accessible.name: placeholderText
}
