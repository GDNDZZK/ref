pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Switch the backend WebSocket endpoint temporarily, save it as default, or restore local discovery.
ModalDialog {
	id: dialog

	property bool tokenCleared: false
	readonly property bool hasExistingToken: Workspace.hasEditableBackendToken && !tokenCleared
	readonly property string sourceLabel: {
		switch (Workspace.backendSource) {
		case "temporary":
			return qsTr("Temporary")
		case "saved":
			return qsTr("Saved default")
		case "direct":
			return qsTr("CLI (--server)")
		case "mock":
			return qsTr("Mock")
		default:
			return qsTr("Local (auto)")
		}
	}

	function validateUrl() {
		const trimmed = urlField.text.trim()
		if (trimmed === "") {
			urlField.errorText = qsTr("Enter a ws:// or wss:// URL.")
			urlField.inputItem.forceActiveFocus()
			return false
		}
		if (!Workspace.isValidBackendUrl(trimmed)) {
			urlField.errorText = qsTr("Invalid URL. Expected ws://host:port or wss://host:port.")
			urlField.inputItem.forceActiveFocus()
			return false
		}
		urlField.errorText = ""
		return true
	}

	function switchTemporarily() {
		if (!validateUrl())
			return
		if (Workspace.switchBackend(urlField.text, tokenField.text, false, hasExistingToken))
			close()
	}

	function saveAsDefault() {
		if (!validateUrl())
			return
		if (Workspace.switchBackend(urlField.text, tokenField.text, true, hasExistingToken))
			close()
	}

	function restoreLocalDefault() {
		Workspace.restoreDefaultBackend()
		close()
	}

	title: qsTr("Switch backend")
	dialogWidth: Theme.settingsWidth
	onAccepted: switchTemporarily()

	Component.onCompleted: {
		urlField.inputItem.forceActiveFocus()
		urlField.selectAll()
	}

	buttons: [
		DialogButton {
			objectName: "backendRestoreButton"
			text: qsTr("Restore local")
			onClicked: dialog.restoreLocalDefault()
		},
		DialogButton {
			objectName: "backendTemporaryButton"
			text: qsTr("Temporary")
			onClicked: dialog.switchTemporarily()
		},
		DialogButton {
			objectName: "backendSaveButton"
			kind: DialogButton.Primary
			text: qsTr("Save as default")
			onClicked: dialog.saveAsDefault()
		}
	]

	Column {
		x: Theme.dialogPadding
		width: parent.width - 2 * Theme.dialogPadding
		topPadding: 4
		bottomPadding: 16
		spacing: 14

		Rectangle {
			width: parent.width
			height: currentCol.implicitHeight + 20
			radius: Theme.radius
			color: Theme.background
			border.color: Theme.border

			Column {
				id: currentCol
				x: 12
				y: 10
				width: parent.width - 24
				spacing: 4

				Item {
					width: parent.width
					height: Math.max(currentLabel.implicitHeight, sourceBadge.height)

					Text {
						id: currentLabel
						textFormat: Text.PlainText
						anchors { left: parent.left; right: sourceBadge.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
						text: qsTr("Current backend")
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
						elide: Text.ElideRight
					}

					Rectangle {
						id: sourceBadge
						anchors { right: parent.right; verticalCenter: parent.verticalCenter }
						width: badgeText.implicitWidth + 14
						height: 20
						radius: 10
						color: Theme.surfaceHover
						border.color: Theme.border

						Text {
							id: badgeText
							textFormat: Text.PlainText
							anchors.centerIn: parent
							text: dialog.sourceLabel
							color: Theme.text
							font.pixelSize: Theme.microFontSize
						}
					}
				}

				Text {
					objectName: "backendCurrentUrlText"
					textFormat: Text.PlainText
					width: parent.width
					text: Workspace.backendUrl !== "" ? Workspace.backendUrl : qsTr("Local automatic discovery")
					color: Theme.text
					font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
					elide: Text.ElideMiddle
				}

				Text {
					textFormat: Text.PlainText
					visible: Workspace.maskedBackendToken !== ""
					width: parent.width
					text: qsTr("Token: %1").arg(Workspace.maskedBackendToken)
					color: Theme.textMuted
					font { family: Theme.monoFamily; pixelSize: Theme.smallFontSize }
					elide: Text.ElideRight
				}
			}
		}

		Column {
			width: parent.width
			spacing: 6

			Text {
				textFormat: Text.PlainText
				text: qsTr("WebSocket URL")
				color: Theme.textMuted
				font.pixelSize: Theme.smallFontSize
			}

			DialogField {
				id: urlField
				objectName: "backendUrlField"
				width: parent.width
				text: Workspace.editableBackendUrl
				placeholderText: qsTr("ws://127.0.0.1:4000/")
				onEdited: errorText = ""
				onAccepted: dialog.switchTemporarily()
			}
		}

		Column {
			width: parent.width
			spacing: 6

			Item {
				width: parent.width
				height: Math.max(tokenLabel.implicitHeight, clearTokenBtn.height)

				Text {
					id: tokenLabel
					textFormat: Text.PlainText
					anchors { left: parent.left; verticalCenter: parent.verticalCenter }
					text: qsTr("Bearer token (optional)")
					color: Theme.textMuted
					font.pixelSize: Theme.smallFontSize
				}

				TextButton {
					id: clearTokenBtn
					objectName: "backendClearTokenButton"
					anchors { right: parent.right; verticalCenter: parent.verticalCenter }
					visible: dialog.hasExistingToken && tokenField.text === ""
					text: qsTr("Clear token")
					onClicked: {
						tokenField.text = ""
						dialog.tokenCleared = true
					}
				}
			}

			SecretField {
				id: tokenField
				objectName: "backendTokenField"
				width: parent.width
				placeholderText: dialog.hasExistingToken
					? qsTr("Leave empty to keep %1").arg(Workspace.maskedEditableBackendToken)
					: qsTr("Optional token")
				onEdited: dialog.tokenCleared = false
				onAccepted: dialog.switchTemporarily()
			}
		}
	}
}
