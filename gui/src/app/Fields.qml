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

	// True when `value` is a JS Array or a Qt QML sequence (e.g. QVariantList / QStringList in QVariant).
	function isArray(value) {
		if (Array.isArray(value))
			return true
		return value !== null
			&& typeof value === "object"
			&& (value instanceof Array || (typeof value.length === "number" && typeof value.splice === "function"))
	}

	// Converts a JS Array or Qt QML sequence into a genuine JS Array (returns [] otherwise).
	function toArray(value) {
		if (Array.isArray(value))
			return value
		if (!isArray(value))
			return []
		const out = []
		for (let i = 0; i < value.length; ++i)
			out.push(value[i])
		return out
	}
}
