import QtQuick
import RefGui

// Short notices in the top right. Each toast leaves by itself after a time that depends on
// its level, stays while the pointer is on it, and goes at a click. The same notice again
// restarts the one that is shown.
Item {
	id: host

	property int lastId: 0

	function show(level, text) {
		for (let i = 0; i < toasts.count; ++i) {
			const toast = toasts.get(i)
			if (toast.level === level && toast.text === text && !toast.leaving) {
				toasts.setProperty(i, "repeats", toast.repeats + 1)
				return
			}
		}
		let shown = 0
		for (let i = 0; i < toasts.count; ++i) {
			if (!toasts.get(i).leaving)
				++shown
		}
		// The oldest leave early when the stack is full.
		for (let i = 0; i < toasts.count && shown >= Theme.toastMax; ++i) {
			if (!toasts.get(i).leaving) {
				toasts.setProperty(i, "leaving", true)
				--shown
			}
		}
		toasts.append({toastId: ++lastId, level: level, text: text, repeats: 0, leaving: false})
	}

	function dismiss(toastId) {
		for (let i = 0; i < toasts.count; ++i) {
			if (toasts.get(i).toastId === toastId) {
				toasts.setProperty(i, "leaving", true)
				return
			}
		}
	}

	function forget(toastId) {
		for (let i = 0; i < toasts.count; ++i) {
			if (toasts.get(i).toastId === toastId) {
				toasts.remove(i)
				return
			}
		}
	}

	Connections {
		target: Workspace
		function onToastRequested(level, text) {
			host.show(level, text)
		}
	}

	ListModel { id: toasts }

	Column {
		anchors { top: parent.top; topMargin: 62; right: parent.right; rightMargin: 14 }
		width: Theme.toastWidth

		Repeater {
			model: toasts

			delegate: Item {
				id: toast

				required property int toastId
				required property string level
				required property string text
				required property int repeats
				required property bool leaving

				readonly property color fill: level === "error" ? Theme.errorSurface : level === "warn" ? Theme.warningSurface : Theme.surface
				readonly property color edge: level === "error" ? Theme.errorBorder : level === "warn" ? Theme.warningBorder : Theme.border
				readonly property color tint: level === "error" ? Theme.danger : level === "warn" ? Theme.warning : Theme.accent
				readonly property int lifetime: level === "error" ? Motion.toastErrorMs : level === "warn" ? Motion.toastWarnMs : Motion.toastInfoMs

				width: parent.width
				height: leaving ? 0 : card.height + 8
				opacity: leaving ? 0 : 1
				clip: true

				Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
				Behavior on opacity {
					NumberAnimation {
						duration: Motion.standard
						easing.type: Motion.ease
						onRunningChanged: if (!running && toast.leaving) host.forget(toast.toastId)
					}
				}

				onRepeatsChanged: {
					life.restart()
					icon.play()
				}

				Timer {
					id: life
					interval: toast.lifetime
					running: !hover.hovered && !toast.leaving
					onTriggered: host.dismiss(toast.toastId)
				}

				Rectangle {
					id: card
					width: parent.width
					height: Math.max(icon.height, message.height) + 2 * 10
					radius: Theme.radius
					color: toast.fill
					border.color: toast.edge
					x: entered ? 0 : Motion.toastSlide

					property bool entered: false

					Behavior on x { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
					Component.onCompleted: {
						entered = true
						icon.enter()
					}

					Item {
						id: icon
						x: 12
						y: 10
						width: 16
						height: 16

						function enter() {
							flourish.enter()
						}
						function play() {
							flourish.play()
						}

						Flourish {
							id: flourish
							target: glyph
							color: toast.tint
						}
						Icon {
							id: glyph
							name: toast.level === "error" ? "error" : toast.level === "warn" ? "warning" : "info"
							size: 16
							color: toast.tint
						}
					}

					Text {
						textFormat: Text.PlainText
						id: message
						x: 12 + 16 + 10
						y: 10
						width: parent.width - x - 12
						text: toast.text
						color: Theme.text
						font.pixelSize: Theme.smallFontSize + 1
						wrapMode: Text.Wrap
						maximumLineCount: 4
						elide: Text.ElideRight
					}
				}

				HoverHandler { id: hover }
				TapHandler { onTapped: host.dismiss(toast.toastId) }
			}
		}
	}
}
