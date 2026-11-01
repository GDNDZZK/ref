pragma ComponentBehavior: Bound

import QtQuick
import RefGui

// What a tool card shows when it is open, laid out by the tool. A tool whose result lacks
// what its layout needs falls back to the plain input and output.
Item {
	id: body

	required property var call

	signal showAllRequested()

	readonly property var meta: call.meta
	readonly property int horizontalPadding: Theme.toolBodyPaddingX
	readonly property int verticalPadding: Theme.toolBodyPaddingY
	readonly property bool hasLines: ToolInfo.listing(call.output).some(group => group.rows.length > 0)
	readonly property string genericFile: "GenericBody.qml"
	// The body of each tool that has its own; edit, write, read and the session tools are decided in fileFor().
	readonly property var bodyFiles: ({
		"bash": "BashBody.qml",
		"grep": "GrepBody.qml",
		"find": "FindBody.qml",
		"webfetch": "WebfetchBody.qml",
		"todo": "TodoBody.qml"
	})
	readonly property string bodyFile: fileFor(call.toolName)

	function fileFor(toolName) {
		if (toolName === "edit" || toolName === "write")
			return Fields.stringOf(meta, "diff") !== "" ? "DiffBody.qml" : genericFile
		if (toolName === "read")
			return hasLines && !call.failed && call.toolState === ToolState.done ? "ReadBody.qml" : genericFile
		if (SessionTools.isSessionTool(toolName))
			return "SessionBody.qml"
		return bodyFiles[toolName] ?? genericFile
	}

	// A body needs `call` as soon as it is made, so it goes in as an initial property.
	function loadBody() {
		loader.setSource(bodyFile, {call: call})
	}

	onBodyFileChanged: loadBody()
	Component.onCompleted: loadBody()

	width: parent ? parent.width : 0
	height: (loader.item ? (loader.item as Item).height : 0) + (call.images.length > 0 ? pictures.height : 0)

	ImageStrip {
		id: pictures
		x: body.horizontalPadding
		y: loader.height
		width: parent.width - 2 * body.horizontalPadding
		visible: body.call.images.length > 0
		images: body.call.images
		sessionId: body.call.chat.sessionId
		onImageClicked: image => body.call.chat.imageClicked(image)
		bottomPadding: body.verticalPadding + Theme.toolBodyBottomExtra
	}

	Loader {
		id: loader
		width: parent.width
		onLoaded: {
			if (item.showAllRequested)
				item.showAllRequested.connect(body.showAllRequested)
		}
	}
}
