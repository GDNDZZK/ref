pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Two-column stash group manager: running sessions on the left, stashed groups on the right.
// Supports button/menu actions and pointer drag-and-drop (dragging members to Running restores; dragging outside both columns removes).
ModalDialog {
	id: dialog

	readonly property int stashDialogWidth: 820
	readonly property real dragThresholdPx: 5
	readonly property real runningColumnRatio: 0.42
	readonly property int columnSpacing: 14
	readonly property int columnPadding: 10
	readonly property int minRootHeight: 320
	readonly property int minColumnHeight: 300
	readonly property int listSpacing: 8
	readonly property int groupInnerSpacing: 6
	readonly property int groupCardPadX: 10
	readonly property int groupCardPadY: 8
	readonly property int headerRowHeight: 28
	readonly property int runningRowHeight: 48
	readonly property int memberRowHeight: 46
	readonly property int tintBarWidth: 3
	readonly property int tintBarInsetY: 6
	readonly property int tintBarRadius: 2
	readonly property int tintDotSize: 7
	readonly property int tintDotX: 10
	readonly property int rowTextLeftMargin: 24
	readonly property int titleEditHeight: 26
	readonly property int dropIndicatorHeight: 2
	readonly property int dragChipMaxWidth: 220
	readonly property int dragChipHeight: 30
	readonly property int dragChipPadX: 12
	readonly property int menuAnchorGap: 4
	readonly property int newGroupPadX: 18
	readonly property int emptyLabelPadY: 20
	readonly property int emptyGroupPadY: 6
	readonly property int emptyGroupsListPadY: 24
	readonly property real draggedOpacity: 0.45
	readonly property real draggedGroupOpacity: 0.55
	readonly property real headerLetterSpacing: 0.6

	property string dragKind: "" // "", "running", "member", "group"
	property string dragSessionId: ""
	property string dragGroupId: ""
	property string dragTitle: ""
	property real dragSceneX: 0
	property real dragSceneY: 0
	property string dropTargetKind: "" // "", "newGroup", "group", "member", "restore", "outside"
	property string dropGroupId: ""
	property string dropMemberSessionId: ""
	property int dropIndex: -1
	property string editingGroupId: ""
	property int groupsRevision: 0
	property var groupsById: ({})

	signal groupsSynced()

	readonly property bool dragging: dragKind !== ""

	function memberTint(parentId, fallbackId) {
		const key = parentId !== "" ? parentId : fallbackId
		return key !== "" ? Theme.hashColor(key) : Theme.accent
	}

	function syncKeyModel(listModel, items, keyName) {
		const wantedKeys = []
		const wantedMap = {}
		for (let i = 0; i < items.length; ++i) {
			const k = String(items[i][keyName] ?? "")
			if (k !== "" && !wantedMap[k]) {
				wantedKeys.push(k)
				wantedMap[k] = true
			}
		}
		for (let i = listModel.count - 1; i >= 0; --i) {
			if (!wantedMap[listModel.get(i).itemId])
				listModel.remove(i)
		}
		for (let i = 0; i < wantedKeys.length; ++i) {
			const k = wantedKeys[i]
			if (i < listModel.count && listModel.get(i).itemId === k)
				continue
			let found = -1
			for (let j = i + 1; j < listModel.count; ++j) {
				if (listModel.get(j).itemId === k) {
					found = j
					break
				}
			}
			if (found >= 0)
				listModel.move(found, i, 1)
			else
				listModel.insert(i, { itemId: k })
		}
	}

	function syncGroupsModel() {
		const groups = Workspace.stash.groups
		const map = {}
		for (let i = 0; i < groups.length; ++i) {
			const g = groups[i]
			map[String(g.id ?? "")] = g
		}
		groupsById = map
		++groupsRevision
		syncKeyModel(groupsListModel, groups, "id")
		groupsSynced()
	}

	function groupInfo(groupId) {
		void groupsRevision
		return groupsById[groupId] ?? null
	}

	function membersForGroup(groupId) {
		const g = groupInfo(groupId)
		return g ? (g.members ?? []) : []
	}

	function memberInfo(groupId, sessionId) {
		const members = membersForGroup(groupId)
		for (let i = 0; i < members.length; ++i) {
			if (String(members[i].sessionId ?? "") === sessionId)
				return members[i]
		}
		return null
	}

	function pointInItem(item, sceneX, sceneY) {
		if (!item || !item.visible)
			return false
		const pt = item.mapFromItem(null, sceneX, sceneY)
		return pt.x >= 0 && pt.y >= 0 && pt.x <= item.width && pt.y <= item.height
	}

	function clearDrag() {
		dragKind = ""
		dragSessionId = ""
		dragGroupId = ""
		dragTitle = ""
		dropTargetKind = ""
		dropGroupId = ""
		dropMemberSessionId = ""
		dropIndex = -1
	}

	function startDrag(kind, sessionId, groupId, title, sceneX, sceneY) {
		dragKind = kind
		dragSessionId = sessionId
		dragGroupId = groupId
		dragTitle = title
		dragSceneX = sceneX
		dragSceneY = sceneY
		updateDropTarget(sceneX, sceneY)
	}

	function updateDropTarget(sceneX, sceneY) {
		dragSceneX = sceneX
		dragSceneY = sceneY
		dropTargetKind = ""
		dropGroupId = ""
		dropMemberSessionId = ""
		dropIndex = -1
		if (!dragging)
			return

		if (dragKind !== "group" && pointInItem(newGroupZone, sceneX, sceneY)) {
			dropTargetKind = "newGroup"
			return
		}

		for (let i = 0; i < groupsListView.count; ++i) {
			const groupCard = groupsListView.itemAtIndex(i)
			if (!groupCard || groupCard.removing || groupCard.groupLocked)
				continue
			if (dragKind === "group" && groupCard.groupId === dragGroupId)
				continue
			if (!pointInItem(groupCard, sceneX, sceneY))
				continue

			if (dragKind !== "group") {
				const memberHit = groupCard.memberHitAt(sceneX, sceneY)
				if (memberHit) {
					dropTargetKind = "member"
					dropGroupId = groupCard.groupId
					dropMemberSessionId = memberHit.sessionId
					dropIndex = memberHit.positionIndex
					return
				}
			}

			dropTargetKind = "group"
			dropGroupId = groupCard.groupId
			return
		}

		if (dragKind === "member") {
			if (pointInItem(runningColumn, sceneX, sceneY))
				dropTargetKind = "restore"
			else if (!pointInItem(groupsColumn, sceneX, sceneY))
				dropTargetKind = "outside"
		}
	}

	function groupMemberCount(groupId) {
		return membersForGroup(groupId).length
	}

	function finishDrag() {
		if (!dragging)
			return
		const kind = dragKind
		const sessionId = dragSessionId
		const sourceGroupId = dragGroupId
		const targetKind = dropTargetKind
		const targetGroupId = dropGroupId
		const targetIndex = dropIndex
		clearDrag()

		if (targetKind === "newGroup") {
			if (kind === "running")
				Workspace.stash.createGroup("", [sessionId], true)
			else if (kind === "member")
				Workspace.stash.createGroupFromMember(sourceGroupId, sessionId)
			return
		}
		if (targetKind === "group") {
			if (kind === "running")
				Workspace.stash.addToGroup(targetGroupId, sessionId, -1, true)
			else if (kind === "member")
				Workspace.stash.moveGroupMember(sourceGroupId, targetGroupId, sessionId, groupMemberCount(targetGroupId), true)
			else if (kind === "group" && sourceGroupId !== targetGroupId)
				Workspace.stash.mergeGroups(sourceGroupId, targetGroupId)
			return
		}
		if (targetKind === "member") {
			if (kind === "running")
				Workspace.stash.addToGroup(targetGroupId, sessionId, targetIndex, true)
			else if (kind === "member")
				Workspace.stash.moveGroupMember(sourceGroupId, targetGroupId, sessionId, targetIndex, true)
			return
		}
		if (targetKind === "restore" && kind === "member") {
			Workspace.stash.restoreGroupMember(sourceGroupId, sessionId)
			return
		}
		if (targetKind === "outside" && kind === "member")
			Workspace.stash.removeFromGroup(sourceGroupId, sessionId)
	}

	function openAssignMenu(anchorItem, sessionId, sourceGroupId) {
		const items = []
		const groups = Workspace.stash.groups
		for (let i = 0; i < groups.length; ++i) {
			const group = groups[i]
			if (group.locked || group.id === sourceGroupId)
				continue
			if (sourceGroupId !== "") {
				items.push({
					text: qsTr("Move to \"%1\"").arg(group.name),
					action: "move",
					groupId: group.id,
					sessionId: sessionId,
					sourceGroupId: sourceGroupId,
					position: group.members.length
				})
				items.push({
					text: qsTr("Copy to \"%1\"").arg(group.name),
					action: "copy",
					groupId: group.id,
					sessionId: sessionId,
					sourceGroupId: sourceGroupId,
					position: -1
				})
			} else {
				items.push({
					text: qsTr("Add to \"%1\"").arg(group.name),
					action: "add",
					groupId: group.id,
					sessionId: sessionId,
					sourceGroupId: "",
					position: -1
				})
			}
		}
		items.push({
			text: qsTr("New group with session"),
			action: "newGroup",
			groupId: "",
			sessionId: sessionId,
			sourceGroupId: sourceGroupId,
			position: 0
		})
		assignMenu.entries = items
		const pt = anchorItem.mapToItem(columnsRoot, 0, anchorItem.height + dialog.menuAnchorGap)
		assignMenu.openAt(pt)
	}

	title: qsTr("Stashed groups")
	dialogWidth: stashDialogWidth

	ListModel { id: groupsListModel }

	Connections {
		target: Workspace.stash
		function onGroupsChanged() { dialog.syncGroupsModel() }
	}

	Component.onCompleted: {
		syncGroupsModel()
		Workspace.stash.refresh()
	}

	Item {
		id: columnsRoot
		x: Theme.dialogPadding
		width: parent.width - 2 * Theme.dialogPadding
		height: Math.max(dialog.minRootHeight, columnsRow.implicitHeight)

		Row {
			id: columnsRow
			width: parent.width
			spacing: dialog.columnSpacing
			readonly property real syncedColumnHeight: Math.max(dialog.minColumnHeight, runningCol.implicitHeight + 2 * dialog.columnPadding, groupsCol.implicitHeight + 2 * dialog.columnPadding)

			// Left column: currently running sessions.
			Rectangle {
				id: runningColumn
				objectName: "stashRunningColumn"
				width: Math.floor((parent.width - parent.spacing) * dialog.runningColumnRatio)
				height: columnsRow.syncedColumnHeight
				radius: Theme.radius
				color: Theme.background
				border.color: dialog.dropTargetKind === "restore" ? Theme.accentText : Theme.border
				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Column {
					id: runningCol
					x: dialog.columnPadding
					y: dialog.columnPadding
					width: parent.width - 2 * dialog.columnPadding
					spacing: dialog.listSpacing

					Item {
						width: parent.width
						height: dialog.headerRowHeight

						Text {
							textFormat: Text.PlainText
							anchors { left: parent.left; right: stashAllBtn.visible ? stashAllBtn.left : parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
							text: qsTr("RUNNING")
							color: Theme.textMuted
							font { pixelSize: Theme.smallFontSize; bold: true; letterSpacing: dialog.headerLetterSpacing }
							elide: Text.ElideRight
						}

						DialogButton {
							id: stashAllBtn
							objectName: "stashDialogStashAllButton"
							anchors { right: parent.right; verticalCenter: parent.verticalCenter }
							small: true
							visible: Workspace.sessions.count > 0
							text: qsTr("Stash all")
							onClicked: Workspace.stash.stashAll()
						}
					}

					Text {
						textFormat: Text.PlainText
						visible: Workspace.sessions.count === 0
						width: parent.width
						topPadding: dialog.emptyLabelPadY
						bottomPadding: dialog.emptyLabelPadY
						horizontalAlignment: Text.AlignHCenter
						text: qsTr("No running sessions")
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}

					ListView {
						id: runningListView
						width: parent.width
						height: contentHeight
						interactive: false
						spacing: dialog.listSpacing
						model: Workspace.sessions

						add: Transition {
							NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
						}
						remove: Transition {
							ParallelAnimation {
								NumberAnimation { property: "opacity"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
								NumberAnimation { property: "height"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
							}
						}
						displaced: Transition {
							NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
						}

						delegate: Rectangle {
							id: runningCard
							required property string sessionId
							required property string displayTitle
							required property string dirName
							required property string parentId
							required property int index

							readonly property color tint: dialog.memberTint(parentId, sessionId)
							readonly property bool beingDragged: dialog.dragKind === "running" && dialog.dragSessionId === sessionId

							width: runningListView.width
							height: dialog.runningRowHeight
							clip: true
							radius: Theme.radiusSmall
							color: Theme.surface
							border.color: beingDragged ? Theme.accentText : Theme.border
							opacity: beingDragged ? dialog.draggedOpacity : 1

							Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

							Rectangle {
								x: 0
								y: dialog.tintBarInsetY
								width: dialog.tintBarWidth
								height: parent.height - 2 * dialog.tintBarInsetY
								radius: dialog.tintBarRadius
								color: runningCard.tint
							}

							Rectangle {
								x: dialog.tintDotX
								anchors.verticalCenter: parent.verticalCenter
								width: dialog.tintDotSize
								height: dialog.tintDotSize
								radius: height / 2
								color: runningCard.tint
							}

							Column {
								anchors { left: parent.left; leftMargin: dialog.rowTextLeftMargin; right: addBtn.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
								spacing: 1

								Text {
									textFormat: Text.PlainText
									width: parent.width
									text: runningCard.displayTitle
									color: Theme.text
									font { pixelSize: Theme.smallFontSize; bold: true }
									elide: Text.ElideRight
								}

								Text {
									textFormat: Text.PlainText
									width: parent.width
									text: runningCard.dirName
									color: Theme.textMuted
									font.pixelSize: Theme.microFontSize
									elide: Text.ElideRight
								}
							}

							MouseArea {
								id: runningDragArea
								anchors { left: parent.left; top: parent.top; bottom: parent.bottom; right: addBtn.left }
								cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
								property real pressX: 0
								property real pressY: 0
								property bool started: false

								onPressed: mouse => {
									pressX = mouse.x
									pressY = mouse.y
									started = false
								}
								onPositionChanged: mouse => {
									const pt = mapToItem(null, mouse.x, mouse.y)
									if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= dialog.dragThresholdPx) {
										started = true
										dialog.startDrag("running", runningCard.sessionId, "", runningCard.displayTitle, pt.x, pt.y)
									} else if (started) {
										dialog.updateDropTarget(pt.x, pt.y)
									}
								}
								onReleased: {
									if (started)
										dialog.finishDrag()
									started = false
								}
								onCanceled: {
									if (started)
										dialog.clearDrag()
									started = false
								}
							}

							IconButton {
								id: addBtn
								anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
								buttonSize: 26
								iconName: "plus"
								iconSize: 16
								tooltipText: qsTr("Add to group")
								Accessible.name: qsTr("Add to group")
								onClicked: dialog.openAssignMenu(addBtn, runningCard.sessionId, "")
							}
						}
					}
				}
			}

			// Right column: stashed groups.
			Rectangle {
				id: groupsColumn
				width: parent.width - runningColumn.width - parent.spacing
				height: columnsRow.syncedColumnHeight
				radius: Theme.radius
				color: Theme.background
				border.color: Theme.border

				Column {
					id: groupsCol
					x: dialog.columnPadding
					y: dialog.columnPadding
					width: parent.width - 2 * dialog.columnPadding
					spacing: dialog.listSpacing

					Item {
						width: parent.width
						height: dialog.headerRowHeight

						Text {
							textFormat: Text.PlainText
							anchors { left: parent.left; right: newGroupZone.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
							text: qsTr("STASHED GROUPS")
							color: Theme.textMuted
							font { pixelSize: Theme.smallFontSize; bold: true; letterSpacing: dialog.headerLetterSpacing }
							elide: Text.ElideRight
						}

						Rectangle {
							id: newGroupZone
							objectName: "newStashGroupButton"
							anchors { right: parent.right; verticalCenter: parent.verticalCenter }
							width: newGroupRow.implicitWidth + dialog.newGroupPadX
							height: dialog.headerRowHeight
							radius: height / 2
							color: (newGroupHover.hovered || dialog.dropTargetKind === "newGroup") ? Theme.surfaceHover : Theme.surface
							border.color: dialog.dropTargetKind === "newGroup" ? Theme.accentText : Theme.border

							Behavior on color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }
							Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							Row {
								id: newGroupRow
								anchors.centerIn: parent
								spacing: 4

								Icon {
									anchors.verticalCenter: parent.verticalCenter
									name: "plus"
									size: 16
									color: Theme.textMuted
								}

								Text {
									textFormat: Text.PlainText
									anchors.verticalCenter: parent.verticalCenter
									text: qsTr("New group")
									color: Theme.text
									font.pixelSize: Theme.smallFontSize
								}
							}

							HoverHandler { id: newGroupHover; cursorShape: Qt.PointingHandCursor }
							TapHandler { onTapped: Workspace.stash.createGroup("", [], false) }
						}
					}

					Text {
						textFormat: Text.PlainText
						visible: Workspace.stash.groupCount === 0
						width: parent.width
						topPadding: dialog.emptyGroupsListPadY
						bottomPadding: dialog.emptyGroupsListPadY
						horizontalAlignment: Text.AlignHCenter
						text: Workspace.stash.loading ? qsTr("Loading…") : qsTr("No stashed groups yet")
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}

					ListView {
						id: groupsListView
						width: parent.width
						height: contentHeight
						interactive: false
						spacing: dialog.listSpacing
						model: groupsListModel

						add: Transition {
							NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
						}
						remove: Transition {
							ParallelAnimation {
								NumberAnimation { property: "opacity"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
								NumberAnimation { property: "height"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
							}
						}
						displaced: Transition {
							NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
						}

						delegate: Rectangle {
							id: groupCard
							required property string itemId
							required property int index

							property bool removing: false
							ListView.onRemove: removing = true

							readonly property string groupId: itemId
							readonly property var groupData: dialog.groupInfo(groupId)
							readonly property string groupName: groupData ? String(groupData.name ?? "") : ""
							readonly property bool groupLocked: groupData ? groupData.locked === true : false
							readonly property var members: groupData ? (groupData.members ?? []) : []
							readonly property bool editing: dialog.editingGroupId === groupId
							readonly property bool isDropTarget: dialog.dropTargetKind === "group" && dialog.dropGroupId === groupId
							readonly property bool beingDragged: dialog.dragKind === "group" && dialog.dragGroupId === groupId

							ListModel { id: membersListModel }

							function syncMembers() {
								dialog.syncKeyModel(membersListModel, members, "sessionId")
							}

							Connections {
								target: dialog
								function onGroupsSynced() { groupCard.syncMembers() }
							}

							Component.onCompleted: syncMembers()

							function memberHitAt(sceneX, sceneY) {
								for (let j = 0; j < membersListView.count; ++j) {
									const rowItem = membersListView.itemAtIndex(j)
									if (rowItem && !rowItem.removing && dialog.pointInItem(rowItem, sceneX, sceneY))
										return { sessionId: rowItem.sessionId, positionIndex: rowItem.index }
								}
								return null
							}

							function commitRename() {
								if (!editing)
									return
								dialog.editingGroupId = ""
								const trimmed = titleEdit.text.trim()
								if (trimmed !== "" && trimmed !== groupName)
									Workspace.stash.renameGroup(groupId, trimmed)
							}

							width: groupsListView.width
							height: groupCol.implicitHeight + 2 * dialog.groupCardPadY
							clip: true
							radius: Theme.radius
							color: Theme.surface
							border.color: (isDropTarget || beingDragged) ? Theme.accentText : Theme.border
							opacity: beingDragged ? dialog.draggedGroupOpacity : 1
							Behavior on height {
								enabled: !groupCard.removing
								NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
							}
							Behavior on opacity {
								enabled: !groupCard.removing
								NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
							}
							Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							Column {
								id: groupCol
								x: dialog.groupCardPadX
								y: dialog.groupCardPadY
								width: parent.width - 2 * dialog.groupCardPadX
								spacing: dialog.groupInnerSpacing

								Item {
									id: groupHeader
									width: parent.width
									height: dialog.headerRowHeight

									MouseArea {
										anchors { left: parent.left; right: groupActions.left; top: parent.top; bottom: parent.bottom }
										enabled: !groupCard.groupLocked && !groupCard.editing
										cursorShape: pressed ? Qt.ClosedHandCursor : Qt.PointingHandCursor
										property real pressX: 0
										property real pressY: 0
										property bool started: false

										onPressed: mouse => {
											pressX = mouse.x
											pressY = mouse.y
											started = false
										}
										onPositionChanged: mouse => {
											const pt = mapToItem(null, mouse.x, mouse.y)
											if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= dialog.dragThresholdPx) {
												started = true
												dialog.startDrag("group", "", groupCard.groupId, groupCard.groupName, pt.x, pt.y)
											} else if (started) {
												dialog.updateDropTarget(pt.x, pt.y)
											}
										}
										onReleased: {
											if (started) {
												dialog.finishDrag()
											} else {
												dialog.editingGroupId = groupCard.groupId
												titleEdit.text = groupCard.groupName
												Qt.callLater(() => {
													titleEdit.forceActiveFocus()
													titleEdit.selectAll()
												})
											}
											started = false
										}
										onCanceled: {
											if (started)
												dialog.clearDrag()
											started = false
										}
									}

									Text {
										textFormat: Text.PlainText
										visible: !groupCard.editing
										anchors { left: parent.left; right: groupActions.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
										text: groupCard.groupName
										color: Theme.text
										font { pixelSize: Theme.secondaryFontSize; bold: true }
										elide: Text.ElideRight
									}

									Rectangle {
										visible: groupCard.editing
										anchors { left: parent.left; right: groupActions.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
										height: dialog.titleEditHeight
										radius: Theme.radiusSmall
										color: Theme.background
										border.color: Theme.accentText

										// TextInput renders plain text only.
										TextInput {
											id: titleEdit
											anchors { fill: parent; leftMargin: 8; rightMargin: 8 }
											verticalAlignment: TextInput.AlignVCenter
											color: Theme.text
											selectionColor: Theme.selection
											selectedTextColor: Theme.text
											font.pixelSize: Theme.secondaryFontSize
											clip: true
											onAccepted: groupCard.commitRename()
											onActiveFocusChanged: if (!activeFocus) groupCard.commitRename()
											Keys.onEscapePressed: event => {
												dialog.editingGroupId = ""
												event.accepted = true
											}
										}
									}

									Row {
										id: groupActions
										anchors { right: parent.right; verticalCenter: parent.verticalCenter }
										spacing: 4

										IconButton {
											anchors.verticalCenter: parent.verticalCenter
											buttonSize: 24
											iconName: "pencil"
											iconSize: 16
											enabled: !groupCard.groupLocked
											opacity: enabled ? 1 : Theme.disabledOpacity
											tooltipText: qsTr("Rename group")
											Accessible.name: qsTr("Rename group")
											onClicked: {
												dialog.editingGroupId = groupCard.groupId
												titleEdit.text = groupCard.groupName
												Qt.callLater(() => {
													titleEdit.forceActiveFocus()
													titleEdit.selectAll()
												})
											}
										}

										IconButton {
											anchors.verticalCenter: parent.verticalCenter
											buttonSize: 24
											iconName: "lock"
											iconSize: 16
											iconColor: groupCard.groupLocked ? Theme.accentText : Theme.textMuted
											tooltipText: groupCard.groupLocked ? qsTr("Unlock group") : qsTr("Lock group")
											Accessible.name: groupCard.groupLocked ? qsTr("Unlock group") : qsTr("Lock group")
											onClicked: Workspace.stash.setGroupLocked(groupCard.groupId, !groupCard.groupLocked)
										}

										DialogButton {
											anchors.verticalCenter: parent.verticalCenter
											small: true
											text: qsTr("Restore all")
											onClicked: Workspace.stash.restoreGroup(groupCard.groupId)
										}

										IconButton {
											anchors.verticalCenter: parent.verticalCenter
											buttonSize: 24
											iconName: "trash"
											iconSize: 16
											enabled: !groupCard.groupLocked
											opacity: enabled ? 1 : Theme.disabledOpacity
											tooltipText: qsTr("Delete group")
											Accessible.name: qsTr("Delete group")
											onClicked: Workspace.stash.deleteGroup(groupCard.groupId)
										}
									}
								}

								Text {
									textFormat: Text.PlainText
									visible: groupCard.members.length === 0
									width: parent.width
									topPadding: dialog.emptyGroupPadY
									bottomPadding: dialog.emptyGroupPadY
									horizontalAlignment: Text.AlignHCenter
									text: qsTr("Drop sessions here")
									color: Theme.textMuted
									font.pixelSize: Theme.smallFontSize
								}

								ListView {
									id: membersListView
									width: parent.width
									height: contentHeight
									interactive: false
									spacing: dialog.groupInnerSpacing
									model: membersListModel

									add: Transition {
										NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Motion.standard; easing.type: Motion.ease }
									}
									remove: Transition {
										ParallelAnimation {
											NumberAnimation { property: "opacity"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
											NumberAnimation { property: "height"; to: 0; duration: Motion.standard; easing.type: Motion.exitEase }
										}
									}
									displaced: Transition {
										NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
									}
									move: Transition {
										NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
									}
									moveDisplaced: Transition {
										NumberAnimation { property: "y"; duration: Motion.standard; easing.type: Motion.ease }
									}

									delegate: Rectangle {
										id: memberRow
										objectName: "stashMemberRow"
										required property string itemId
										required property int index

										property bool removing: false
										ListView.onRemove: removing = true

										readonly property string sessionId: itemId
										readonly property var memberData: dialog.memberInfo(groupCard.groupId, sessionId)
										readonly property string displayTitle: memberData ? String(memberData.displayTitle ?? sessionId) : sessionId
										readonly property string dirName: memberData ? String(memberData.dirName ?? "") : ""
										readonly property bool live: memberData ? memberData.live === true : false
										readonly property string parentId: memberData ? String(memberData.parentId ?? "") : ""
										readonly property color tint: dialog.memberTint(parentId, sessionId)
										readonly property bool isDropTarget: dialog.dropTargetKind === "member"
											&& dialog.dropGroupId === groupCard.groupId
											&& dialog.dropMemberSessionId === sessionId
										readonly property bool beingDragged: dialog.dragKind === "member"
											&& dialog.dragGroupId === groupCard.groupId
											&& dialog.dragSessionId === sessionId

										width: membersListView.width
										height: dialog.memberRowHeight
										clip: true
										radius: Theme.radiusSmall
										color: live ? Theme.userBubble : Theme.background
										border.color: isDropTarget ? Theme.accentText : (live ? Theme.userBubbleBorder : Theme.border)
										opacity: beingDragged ? dialog.draggedOpacity : 1

										Behavior on opacity {
											enabled: !memberRow.removing
											NumberAnimation { duration: Motion.standard; easing.type: Motion.ease }
										}

										Rectangle {
											visible: memberRow.isDropTarget
											anchors { top: parent.top; left: parent.left; right: parent.right }
											height: dialog.dropIndicatorHeight
											color: Theme.accentText
										}

										Rectangle {
											x: 0
											y: dialog.tintBarInsetY
											width: dialog.tintBarWidth
											height: parent.height - 2 * dialog.tintBarInsetY
											radius: dialog.tintBarRadius
											color: memberRow.tint
										}

										Rectangle {
											x: dialog.tintDotX
											anchors.verticalCenter: parent.verticalCenter
											width: dialog.tintDotSize
											height: dialog.tintDotSize
											radius: height / 2
											color: memberRow.tint
										}

										Column {
											anchors { left: parent.left; leftMargin: dialog.rowTextLeftMargin; right: memberActions.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
											spacing: 1

											Text {
												textFormat: Text.PlainText
												width: parent.width
												text: memberRow.displayTitle
												color: Theme.text
												font { pixelSize: Theme.smallFontSize; bold: true }
												elide: Text.ElideRight
											}

											Text {
												textFormat: Text.PlainText
												width: parent.width
												text: memberRow.dirName
												color: Theme.textMuted
												font.pixelSize: Theme.microFontSize
												elide: Text.ElideRight
											}
										}

										MouseArea {
											anchors { left: parent.left; top: parent.top; bottom: parent.bottom; right: memberActions.left }
											enabled: !groupCard.groupLocked
											cursorShape: !groupCard.groupLocked ? (pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor) : Qt.ArrowCursor
											property real pressX: 0
											property real pressY: 0
											property bool started: false

											onPressed: mouse => {
												pressX = mouse.x
												pressY = mouse.y
												started = false
											}
											onPositionChanged: mouse => {
												const pt = mapToItem(null, mouse.x, mouse.y)
												if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= dialog.dragThresholdPx) {
													started = true
													dialog.startDrag("member", memberRow.sessionId, groupCard.groupId, memberRow.displayTitle, pt.x, pt.y)
												} else if (started) {
													dialog.updateDropTarget(pt.x, pt.y)
												}
											}
											onReleased: {
												if (started)
													dialog.finishDrag()
												started = false
											}
											onCanceled: {
												if (started)
													dialog.clearDrag()
												started = false
											}
										}

										Row {
											id: memberActions
											anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
											spacing: 4

											IconButton {
												id: memberAssignBtn
												anchors.verticalCenter: parent.verticalCenter
												buttonSize: 24
												iconName: "plus"
												iconSize: 16
												tooltipText: qsTr("Move or copy to group")
												Accessible.name: qsTr("Move or copy to group")
												onClicked: dialog.openAssignMenu(memberAssignBtn, memberRow.sessionId, groupCard.groupId)
											}

											IconButton {
												anchors.verticalCenter: parent.verticalCenter
												buttonSize: 24
												iconName: "restore"
												iconSize: 20
												tooltipText: qsTr("Restore session")
												Accessible.name: qsTr("Restore session")
												onClicked: Workspace.stash.restoreGroupMember(groupCard.groupId, memberRow.sessionId)
											}

											IconButton {
												anchors.verticalCenter: parent.verticalCenter
												buttonSize: 24
												iconName: "close"
												iconSize: 16
												enabled: !groupCard.groupLocked
												opacity: enabled ? 1 : Theme.disabledOpacity
												tooltipText: qsTr("Remove from group")
												Accessible.name: qsTr("Remove from group")
												onClicked: Workspace.stash.removeFromGroup(groupCard.groupId, memberRow.sessionId)
											}
										}
									}
								}
							}
						}
					}
				}
			}
		}

		// Floating drag chip following the pointer while dragging.
		Rectangle {
			id: dragProxy
			visible: dialog.dragging
			z: 50
			x: {
				const pt = columnsRoot.mapFromItem(null, dialog.dragSceneX, dialog.dragSceneY)
				return pt.x - width / 2
			}
			y: {
				const pt = columnsRoot.mapFromItem(null, dialog.dragSceneX, dialog.dragSceneY)
				return pt.y - height / 2
			}
			width: Math.min(dialog.dragChipMaxWidth, dragProxyText.implicitWidth + 2 * dialog.dragChipPadX)
			height: dialog.dragChipHeight
			radius: height / 2
			color: Theme.userBubble
			border.color: dialog.dropTargetKind === "outside" ? Theme.danger : Theme.accentText

			Text {
				id: dragProxyText
				textFormat: Text.PlainText
				anchors { left: parent.left; right: parent.right; leftMargin: dialog.dragChipPadX; rightMargin: dialog.dragChipPadX; verticalCenter: parent.verticalCenter }
				text: dialog.dropTargetKind === "restore"
					? qsTr("Drop to restore: %1").arg(dialog.dragTitle)
					: (dialog.dropTargetKind === "outside"
						? qsTr("Drop to remove: %1").arg(dialog.dragTitle)
						: dialog.dragTitle)
				color: Theme.text
				font.pixelSize: Theme.smallFontSize
				elide: Text.ElideRight
			}
		}

		ContextMenu {
			id: assignMenu
			onTriggered: index => {
				const entry = assignMenu.entries[index]
				if (!entry)
					return
				if (entry.action === "newGroup") {
					if (entry.sourceGroupId !== "")
						Workspace.stash.createGroupFromMember(entry.sourceGroupId, entry.sessionId)
					else
						Workspace.stash.createGroup("", [entry.sessionId], true)
				} else if (entry.action === "add") {
					Workspace.stash.addToGroup(entry.groupId, entry.sessionId, -1, true)
				} else if (entry.action === "copy") {
					Workspace.stash.addToGroup(entry.groupId, entry.sessionId, -1, false)
				} else if (entry.action === "move") {
					Workspace.stash.moveGroupMember(entry.sourceGroupId, entry.groupId, entry.sessionId, entry.position, false)
				}
			}
		}
	}
}
