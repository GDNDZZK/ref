pragma ComponentBehavior: Bound
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

			delegate: Toast {
				onDismissRequested: host.dismiss(toastId)
				onFinished: host.forget(toastId)
			}
		}
	}
}
