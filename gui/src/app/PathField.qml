pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// A path field for a directory or file on the backend's machine, with matching entries listed
// below. Tab or a click takes an entry; Up / Down choose; Enter submits the field.
Item {
	id: path

	property alias text: field.text
	property alias placeholderText: field.placeholderText
	property alias errorText: field.errorText
	property string cwd: ""
	property bool includeFiles: false
	property var entries: []
	readonly property var names: entries.map(entry => entry.name)
	property string listedPath: ""
	property string listError: "" // why the backend could not list the directory, shown in place of the names
	property string cachedKey: ""
	property var cachedDirs: []
	property var cachedFiles: []
	property int request: 0
	readonly property int visibleRows: 6
	readonly property int rowHeight: 32
	readonly property int debounceMs: 150 // wait for a pause in typing before listing a new directory
	readonly property bool completing: entries.length > 0

	signal accepted()

	function select() {
		field.selectAll()
	}

	property bool animateHighlight: false

	function isMissingDirectory(code, listing) {
		if (code === "dir_not_found" || code === "not_found")
			return true
		return code === "" && typeof listing === "string" && listing.startsWith("Directory does not exist")
	}

	function move(delta) {
		if (!completing)
			return
		const base = view.currentIndex < 0 ? 0 : view.currentIndex
		const next = Math.max(0, Math.min(base + delta, entries.length - 1))
		animateHighlight = true
		view.currentIndex = next
		view.positionViewAtIndex(next, ListView.Contain)
	}

	function applyFilter(dirs, files, prefix) {
		const showHidden = prefix.startsWith(".")
		const next = []
		for (const dir of dirs) {
			if ((!dir.startsWith(".") || showHidden) && dir.toLowerCase().startsWith(prefix))
				next.push({name: dir, isDirectory: true})
		}
		if (includeFiles) {
			for (const file of files) {
				if ((!file.startsWith(".") || showHidden) && file.toLowerCase().startsWith(prefix))
					next.push({name: file, isDirectory: false})
			}
		}
		entries = next
		animateHighlight = false
		hoverGuard.reset()
		view.currentIndex = entries.length > 0 ? 0 : -1
		if (entries.length > 0)
			view.positionViewAtBeginning()
	}

	function lastSlashIndex(str) {
		return Math.max(str.lastIndexOf("/"), str.lastIndexOf("\\"))
	}

	function preferredSeparator(str) {
		const idx = lastSlashIndex(str)
		if (idx >= 0)
			return str[idx]
		return /^[A-Za-z]:/.test(str) ? "\\" : "/"
	}

	function isAbsOrHomePath(str) {
		return str.startsWith("/")
			|| str.startsWith("\\")
			|| /^[A-Za-z]:/.test(str)
			|| str === "~"
			|| str.startsWith("~/")
			|| str.startsWith("~\\")
	}

	function parseDirectoryAndPrefix(str) {
		if (str === "~")
			return { directory: "~", prefix: "" }
		if (/^[A-Za-z]:$/.test(str))
			return { directory: str + "\\", prefix: "" }
		const slash = lastSlashIndex(str)
		if (slash < 0)
			return { directory: ".", prefix: str.toLowerCase() }
		const rawDir = str.slice(0, slash + 1)
		let directory = rawDir
		if (rawDir === "./" || rawDir === ".\\")
			directory = "."
		else if (rawDir === "~/" || rawDir === "~\\")
			directory = "~"
		return { directory: directory, prefix: str.slice(slash + 1).toLowerCase() }
	}

	// The part up to the last slash/backslash is the directory to list; the rest filters its names.
	function refresh() {
		const parsed = parseDirectoryAndPrefix(text)
		const directory = parsed.directory
		const prefix = parsed.prefix
		const key = cwd + "\0" + directory + "\0" + (includeFiles ? "1" : "0")

		if (cachedKey === key && listError === "") {
			applyFilter(cachedDirs, cachedFiles, prefix)
			return
		}

		const mine = ++request
		Workspace.listDirs(directory, (ok, listing, code) => {
			if (mine !== request)
				return
			const notFound = !ok && isMissingDirectory(code, listing)
			listedPath = ok ? listing.path : ""
			listError = ok || notFound ? "" : listing
			cachedKey = ok ? key : ""
			cachedDirs = ok ? listing.dirs : []
			cachedFiles = ok ? (listing.files ?? []) : []
			if (ok)
				applyFilter(cachedDirs, cachedFiles, prefix)
			else
				applyFilter([], [], prefix)
		}, cwd, includeFiles)
	}

	function accept(entry) {
		if (!entry)
			return
		const name = typeof entry === "string" ? entry : entry.name
		const isDir = typeof entry === "string" ? true : entry.isDirectory
		const sep = preferredSeparator(text)
		const trail = isDir ? sep : ""
		if (isAbsOrHomePath(text) && listedPath !== "") {
			const normBase = sep === "\\" ? listedPath.replace(/\//g, "\\") : listedPath.replace(/\\/g, "/")
			const strippedBase = (normBase === "/" || normBase === "\\") ? "" : normBase.replace(/[\\/]+$/, "")
			text = strippedBase + sep + name + trail
		} else {
			const slash = lastSlashIndex(text)
			const relPrefix = text === "~"
				? ("~" + sep)
				: (/^[A-Za-z]:$/.test(text) ? (text + sep) : (slash < 0 ? "" : text.slice(0, slash + 1)))
			text = relPrefix + name + trail
		}
		field.inputItem.cursorPosition = text.length
		if (isDir)
			refresh()
		else
			entries = []
	}

	onCwdChanged: {
		cachedKey = ""
		if (completedInit)
			refresh()
	}

	property bool completedInit: false

	implicitHeight: field.implicitHeight + (completing ? 8 + Math.min(entries.length, visibleRows) * rowHeight
		: listError !== "" ? 8 + errorLine.implicitHeight : 0)

	Behavior on implicitHeight { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

	PointerMoveGuard { id: hoverGuard }

	Timer {
		id: debounce
		interval: path.debounceMs
		onTriggered: path.refresh()
	}
	Component.onCompleted: {
		completedInit = true
		refresh()
	}
	Component.onDestruction: request = -1

	DialogField {
		id: field
		width: parent.width
		onAccepted: path.accepted()
		onEdited: {
			const directory = path.parseDirectoryAndPrefix(text).directory
			const key = path.cwd + "\0" + directory + "\0" + (path.includeFiles ? "1" : "0")
			if (path.cachedKey === key && path.listError === "") {
				debounce.stop()
				path.refresh()
			} else {
				debounce.restart()
			}
		}

		onTabPressed: {
			if (view.currentIndex >= 0 && view.currentIndex < path.entries.length)
				path.accept(path.entries[view.currentIndex])
		}
		onDownPressed: path.move(1)
		onUpPressed: path.move(-1)
	}

	Text {
		id: errorLine
		textFormat: Text.PlainText
		y: field.height + 8
		width: parent.width
		text: path.listError
		color: Theme.dangerText
		font.pixelSize: Theme.smallFontSize
		wrapMode: Text.Wrap
		visible: path.listError !== ""
	}

	ListView {
		id: view
		y: field.height + 8
		width: parent.width
		height: Math.min(path.entries.length, path.visibleRows) * path.rowHeight
		clip: true
		model: path.entries
		boundsBehavior: Flickable.StopAtBounds
		highlightFollowsCurrentItem: false
		visible: path.completing

		highlight: Rectangle {
			y: view.currentItem ? view.currentItem.y : 0
			width: view.width
			height: path.rowHeight
			radius: 8
			color: Theme.surfaceHover
			visible: view.currentItem !== null

			Behavior on y {
				enabled: path.animateHighlight
				NumberAnimation { duration: Motion.quick; easing.type: Motion.ease }
			}
		}

		HoverHandler {
			onPointChanged: {
				if (!hovered || !hoverGuard.movedTo(point.scenePosition))
					return
				const local = view.mapFromItem(null, point.scenePosition.x, point.scenePosition.y)
				const index = view.indexAt(local.x + view.contentX, local.y + view.contentY)
				if (index >= 0) {
					path.animateHighlight = true
					view.currentIndex = index
				}
			}
		}
		delegate: Item {
			id: row
			required property var modelData
			required property int index
			width: view.width
			height: path.rowHeight

			Icon {
				x: 12
				anchors.verticalCenter: parent.verticalCenter
				name: row.modelData.isDirectory ? "folder" : "file"
				size: 16
				color: Theme.textMuted
			}
			Text {
				textFormat: Text.PlainText
				anchors { left: parent.left; leftMargin: 38; right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: row.modelData.name + (row.modelData.isDirectory ? path.preferredSeparator(path.text) : "")
				color: Theme.text
				font.pixelSize: Theme.secondaryFontSize
				elide: Text.ElideRight
			}
			HoverHandler {
				cursorShape: Qt.PointingHandCursor
			}
			TapHandler { onTapped: path.accept(row.modelData) }
		}
	}
}
