pragma Singleton
import QtQuick

// Opens links from text we did not write. Only web and mail addresses leave the program: a
// `file:` or custom-scheme link could start something on the machine.
QtObject {
	readonly property var allowedScheme: /^(https?|mailto):/i

	function isAllowed(link) {
		return allowedScheme.test(String(link).trim())
	}

	function open(link) {
		if (isAllowed(link))
			Qt.openUrlExternally(String(link).trim())
	}
}
