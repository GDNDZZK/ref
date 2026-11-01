pragma Singleton
import QtQuick

// Reads a field of a JSON object that may be missing or of another type.
QtObject {
	// The string at object[key], or "" when there is none.
	function stringOf(object, key) {
		return typeof object[key] === "string" ? object[key] : ""
	}

	// Whether object has the key itself, not from Object.prototype ("constructor", "toString").
	function has(object, key) {
		return Object.prototype.hasOwnProperty.call(object, key)
	}

	function hasNumber(object, key) {
		return typeof object[key] === "number"
	}
}
