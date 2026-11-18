import QtQuick
import RefGui

// Smart secret field: masks literal API keys/tokens with a Show/Hide eye toggle, while keeping
// `{env:VAR}` and `{file:...}` interpolation tokens visible in plain text with live env_status badges.
FocusScope {
	id: secret

	property alias text: input.text
	property string placeholderText: qsTr("API key or {env:VAR}")
	property string errorText: ""
	// Map `{ [varName]: bool }` from ConfigFilesInfo.env_status.
	property var envStatus: ({})
	property bool revealed: false

	signal accepted()
	signal edited()

	readonly property bool hasError: errorText !== ""
	readonly property var referencedEnvVars: {
		const found = []
		let rest = input.text
		while (rest.length > 0) {
			const envIdx = rest.indexOf("{env:")
			const fileIdx = rest.indexOf("{file:")
			const start = envIdx < 0 ? fileIdx : (fileIdx < 0 ? envIdx : Math.min(envIdx, fileIdx))
			if (start < 0)
				break
			const slice = rest.slice(start)
			const closeIdx = slice.indexOf("}")
			if (closeIdx < 0)
				break
			const token = slice.slice(0, closeIdx)
			if (token.startsWith("{env:")) {
				const name = token.slice(5).trim()
				if (name !== "" && !found.includes(name))
					found.push(name)
			}
			rest = slice.slice(closeIdx + 1)
		}
		return found
	}
	readonly property bool hasFileToken: /\{file:\s*[^}\s][^}]*\}/.test(input.text)
	readonly property bool isInterpolated: referencedEnvVars.length > 0 || hasFileToken
	readonly property var missingEnvVars: referencedEnvVars.filter(name =>
		Fields.has(envStatus, name) && envStatus[name] === false)
	readonly property bool hasKnownEnvStatus: referencedEnvVars.length > 0
		&& referencedEnvVars.every(name => Fields.has(envStatus, name))
	readonly property bool showEnvBadge: referencedEnvVars.length > 0
		&& (hasKnownEnvStatus || missingEnvVars.length > 0)
	readonly property bool envAllSet: missingEnvVars.length === 0 && hasKnownEnvStatus
	readonly property bool showEyeButton: !isInterpolated && input.text !== ""

	implicitWidth: 260
	implicitHeight: Theme.fieldHeight + (hasError ? errorLabel.implicitHeight + 6 : 0)
	opacity: enabled ? 1 : Theme.disabledOpacity

	Rectangle {
		id: box
		width: parent.width
		height: Theme.fieldHeight
		radius: Theme.radius
		color: Theme.background
		border.color: secret.hasError ? Theme.dangerText : (input.activeFocus ? Theme.accentText : Theme.border)

		Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

		TextInput {
			id: input
			anchors {
				left: parent.left
				leftMargin: 12
				right: trailing.left
				rightMargin: trailing.width > 0 ? 8 : 12
				top: parent.top
				bottom: parent.bottom
			}
			verticalAlignment: TextInput.AlignVCenter
			echoMode: secret.isInterpolated || secret.revealed ? TextInput.Normal : TextInput.Password
			passwordCharacter: "•"
			color: Theme.text
			selectionColor: Theme.selection
			selectedTextColor: Theme.text
			font {
				family: secret.isInterpolated ? Theme.monoFamily : ""
				pixelSize: secret.isInterpolated ? Theme.codeFontSize : Theme.secondaryFontSize
			}
			clip: true
			focus: true
			Accessible.name: secret.placeholderText
			onAccepted: secret.accepted()
			onTextEdited: secret.edited()

			PlaceholderText {
				anchors.fill: parent
				textInput: input
				text: secret.placeholderText
				font.pixelSize: Theme.secondaryFontSize
				elide: Text.ElideRight
			}
		}

		Row {
			id: trailing
			anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
			spacing: 4

			Rectangle {
				id: envPill
				visible: secret.showEnvBadge
				anchors.verticalCenter: parent.verticalCenter
				width: envText.implicitWidth + 14
				height: 22
				radius: 11
				color: secret.envAllSet ? Theme.diffAddedBackground : Theme.warningSurface
				border.color: secret.envAllSet ? Theme.success : Theme.warningBorder

				Text {
					id: envText
					textFormat: Text.PlainText
					anchors.centerIn: parent
					text: secret.envAllSet ? qsTr("✓ set") : qsTr("⚠ unset")
					color: secret.envAllSet ? Theme.successText : Theme.warningText
					font.pixelSize: Theme.microFontSize
				}

				HoverHandler { id: envHover }
				PlainToolTip {
					visible: envHover.hovered
					text: secret.envAllSet
						? qsTr("Environment variable set: %1").arg(secret.referencedEnvVars.join(", "))
						: qsTr("Environment variable unset or empty: %1").arg(secret.missingEnvVars.join(", "))
				}
			}

			IconButton {
				id: eyeBtn
				visible: secret.showEyeButton
				anchors.verticalCenter: parent.verticalCenter
				buttonSize: 26
				iconName: secret.revealed ? "eye-off" : "eye"
				iconSize: 16
				Accessible.name: secret.revealed ? qsTr("Hide secret") : qsTr("Show secret")
				onClicked: secret.revealed = !secret.revealed

				HoverHandler { id: eyeHover }
				PlainToolTip {
					visible: eyeHover.hovered
					text: secret.revealed ? qsTr("Hide secret") : qsTr("Show secret")
				}
			}
		}
	}

	Text {
		id: errorLabel
		textFormat: Text.PlainText
		visible: secret.hasError
		anchors { top: box.bottom; topMargin: 6; left: parent.left; right: parent.right }
		text: secret.errorText
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
	}
}
