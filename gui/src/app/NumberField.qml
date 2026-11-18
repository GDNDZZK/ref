import QtQuick
import RefGui

// Numeric configuration field (integer or decimal) where clearing the input yields `undefined`
// so optional keys in `ref.json` can be omitted cleanly.
FocusScope {
	id: numField

	// `undefined` / `null` when unset, else a number.
	property var value: undefined
	property real minimum: -Infinity
	property real maximum: Infinity
	property real step: 1
	property bool decimals: false
	property string suffix: ""
	property string placeholderText: ""
	property string errorText: ""
	property bool outOfRange: false

	signal valueModified(var newValue)

	readonly property bool hasValue: typeof value === "number" && Number.isFinite(value)
	readonly property string effectiveError: errorText !== ""
		? errorText
		: (outOfRange ? qsTr("Must be between %1 and %2").arg(formatBound(minimum)).arg(formatBound(maximum)) : "")
	readonly property bool hasError: effectiveError !== ""

	function formatBound(bound) {
		if (bound === -Infinity)
			return "-∞"
		if (bound === Infinity)
			return "∞"
		return String(bound)
	}

	function formatValue(val) {
		if (typeof val !== "number" || !Number.isFinite(val))
			return ""
		return decimals ? String(Math.round(val * 1000000) / 1000000) : String(Math.trunc(val))
	}

	function syncFromValue() {
		const formatted = formatValue(value)
		if (input.text !== formatted)
			input.text = formatted
		outOfRange = false
	}

	function parseAndEmit(commitClamp) {
		const raw = input.text.trim()
		if (raw === "") {
			outOfRange = false
			if (value !== undefined) {
				value = undefined
				valueModified(undefined)
			}
			return
		}
		const pattern = decimals ? /^-?(?:\d+\.?\d*|\.\d+)$/ : /^-?\d+$/
		if (!pattern.test(raw)) {
			if (commitClamp)
				syncFromValue()
			return
		}
		let parsed = decimals ? parseFloat(raw) : parseInt(raw, 10)
		if (!Number.isFinite(parsed))
			return
		if (parsed < minimum || parsed > maximum) {
			if (commitClamp) {
				parsed = Math.min(maximum, Math.max(minimum, parsed))
				outOfRange = false
				input.text = formatValue(parsed)
			} else {
				outOfRange = true
				return
			}
		} else {
			outOfRange = false
		}
		if (value !== parsed) {
			value = parsed
			valueModified(parsed)
		}
	}

	function stepBy(direction) {
		if (!enabled)
			return
		const base = hasValue ? value : (Number.isFinite(minimum) && minimum > 0 ? minimum : 0)
		let next = base + direction * step
		if (decimals)
			next = Math.round(next * 1000000) / 1000000
		else
			next = Math.round(next)
		next = Math.min(maximum, Math.max(minimum, next))
		outOfRange = false
		value = next
		input.text = formatValue(next)
		valueModified(next)
	}

	implicitWidth: 180
	implicitHeight: Theme.fieldHeight + (hasError ? errorLabel.implicitHeight + 6 : 0)
	opacity: enabled ? 1 : Theme.disabledOpacity

	onValueChanged: {
		if (!input.activeFocus)
			syncFromValue()
	}
	Component.onCompleted: syncFromValue()

	Rectangle {
		id: box
		width: parent.width
		height: Theme.fieldHeight
		radius: Theme.radius
		color: Theme.background
		border.color: numField.hasError ? Theme.dangerText : (input.activeFocus ? Theme.accentText : Theme.border)

		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		TextInput {
			id: input
			anchors {
				left: parent.left
				leftMargin: 12
				right: trailing.left
				rightMargin: 8
				top: parent.top
				bottom: parent.bottom
			}
			verticalAlignment: TextInput.AlignVCenter
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font { family: Theme.monoFamily; pixelSize: Theme.codeFontSize }
			clip: true
			focus: true
			Accessible.name: numField.placeholderText
			onTextEdited: numField.parseAndEmit(false)
			onEditingFinished: numField.parseAndEmit(true)
			Keys.onUpPressed: event => {
				numField.stepBy(1)
				event.accepted = true
			}
			Keys.onDownPressed: event => {
				numField.stepBy(-1)
				event.accepted = true
			}

			PlaceholderText {
				anchors.fill: parent
				textInput: input
				text: numField.placeholderText
				font.pixelSize: Theme.secondaryFontSize
				elide: Text.ElideRight
			}
		}

		Row {
			id: trailing
			anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
			spacing: 4

			Text {
				textFormat: Text.PlainText
				visible: numField.suffix !== ""
				anchors.verticalCenter: parent.verticalCenter
				text: numField.suffix
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}

			Column {
				anchors.verticalCenter: parent.verticalCenter
				spacing: 0

				Item {
					width: 20
					height: 14

					Chevron {
						anchors.centerIn: parent
						implicitWidth: 10
						implicitHeight: 10
						rotation: -90
						color: upHover.hovered ? Theme.text : Theme.textMuted
					}
					HoverHandler { id: upHover; enabled: numField.enabled; cursorShape: Qt.PointingHandCursor }
					TapHandler { enabled: numField.enabled; onTapped: numField.stepBy(1) }
				}

				Item {
					width: 20
					height: 14

					Chevron {
						anchors.centerIn: parent
						implicitWidth: 10
						implicitHeight: 10
						rotation: 90
						color: downHover.hovered ? Theme.text : Theme.textMuted
					}
					HoverHandler { id: downHover; enabled: numField.enabled; cursorShape: Qt.PointingHandCursor }
					TapHandler { enabled: numField.enabled; onTapped: numField.stepBy(-1) }
				}
			}
		}
	}

	Text {
		id: errorLabel
		textFormat: Text.PlainText
		visible: numField.hasError
		anchors { top: box.bottom; topMargin: 6; left: parent.left; right: parent.right }
		text: numField.effectiveError
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}
}
