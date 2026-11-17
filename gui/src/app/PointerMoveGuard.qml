import QtQuick

// Prevents list hover handlers from hijacking keyboard selection when a popup opens or
// scrolls underneath a stationary mouse cursor; selection updates only after real movement.
QtObject {
	property point lastScenePos: Qt.point(-1, -1)
	property bool primed: false

	function reset() {
		primed = false
	}

	function movedTo(scenePos) {
		if (!primed) {
			lastScenePos = scenePos
			primed = true
			return false
		}
		if (scenePos.x === lastScenePos.x && scenePos.y === lastScenePos.y)
			return false
		lastScenePos = scenePos
		return true
	}
}
