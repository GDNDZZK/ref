pragma ComponentBehavior: Bound
import QtQuick
import RefGui

// Two-column stash group manager: running sessions on the left, stashed groups on the right.
// Supports button/menu actions and pointer drag-and-drop (including dragging members out to remove).
ModalDialog {
	id: dialog

	property string dragKind: "" // "", "running", "member", "group"
	property string dragSessionId: ""
	property string dragGroupId: ""
	property string dragTitle: ""
	property real dragSceneX: 0
	property real dragSceneY: 0
	property string dropTargetKind: "" // "", "newGroup", "group", "member", "outside"
	property string dropGroupId: ""
	property string dropMemberSessionId: ""
	property int dropIndex: -1
	property string editingGroupId: ""

	readonly property bool dragging: dragKind !== ""

	function memberTint(rootId, fallbackId) {
		const key = rootId !== "" ? rootId : fallbackId
		return key !== "" ? Theme.hashColor(key) : Theme.accent
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

		for (let i = 0; i < groupsRepeater.count; ++i) {
			const groupCard = groupsRepeater.itemAt(i)
			if (!groupCard || groupCard.groupLocked)
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

		if (dragKind === "member" && (pointInItem(runningColumn, sceneX, sceneY) || !pointInItem(groupsColumn, sceneX, sceneY)))
			dropTargetKind = "outside"
	}

	function groupMemberCount(groupId) {
		const groups = Workspace.stash.groups
		for (let i = 0; i < groups.length; ++i) {
			if (groups[i].id === groupId)
				return groups[i].members.length
		}
		return 0
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
		const pt = anchorItem.mapToItem(columnsRoot, 0, anchorItem.height + 4)
		assignMenu.openAt(pt)
	}

	title: qsTr("Stashed groups")
	dialogWidth: 820

	Component.onCompleted: Workspace.stash.refresh()

	Item {
		id: columnsRoot
		x: Theme.dialogPadding
		width: parent.width - 2 * Theme.dialogPadding
		height: Math.max(320, columnsRow.implicitHeight)

		Row {
			id: columnsRow
			width: parent.width
			spacing: 14

			// Left column: currently running sessions.
			Rectangle {
				id: runningColumn
				width: Math.floor((parent.width - parent.spacing) * 0.42)
				height: Math.max(300, runningCol.implicitHeight + 20)
				radius: Theme.radius
				color: Theme.background
				border.color: dialog.dropTargetKind === "outside" ? Theme.danger : Theme.border

				Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

				Column {
					id: runningCol
					x: 10
					y: 10
					width: parent.width - 20
					spacing: 8

					Item {
						width: parent.width
						height: 28

						Text {
							textFormat: Text.PlainText
							anchors { left: parent.left; right: stashAllBtn.visible ? stashAllBtn.left : parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
							text: qsTr("RUNNING")
							color: Theme.textMuted
							font { pixelSize: Theme.smallFontSize; bold: true; letterSpacing: 0.6 }
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
						topPadding: 20
						bottomPadding: 20
						horizontalAlignment: Text.AlignHCenter
						text: qsTr("No running sessions")
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}

					Repeater {
						model: Workspace.sessions

						delegate: Rectangle {
							id: runningCard
							required property string sessionId
							required property string displayTitle
							required property string dirName
							required property string rootId
							required property int index

							readonly property color tint: dialog.memberTint(rootId, sessionId)
							readonly property bool beingDragged: dialog.dragKind === "running" && dialog.dragSessionId === sessionId

							width: runningCol.width
							height: 48
							radius: Theme.radiusSmall
							color: Theme.surface
							border.color: beingDragged ? Theme.accentText : Theme.border
							opacity: beingDragged ? 0.45 : 1

							Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

							Rectangle {
								x: 0
								y: 6
								width: 3
								height: parent.height - 12
								radius: 2
								color: runningCard.tint
							}

							Rectangle {
								x: 10
								anchors.verticalCenter: parent.verticalCenter
								width: 7
								height: 7
								radius: 3.5
								color: runningCard.tint
							}

							Column {
								anchors { left: parent.left; leftMargin: 24; right: addBtn.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
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
									if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= 5) {
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
				height: Math.max(300, groupsCol.implicitHeight + 20)
				radius: Theme.radius
				color: Theme.background
				border.color: Theme.border

				Column {
					id: groupsCol
					x: 10
					y: 10
					width: parent.width - 20
					spacing: 8

					Item {
						width: parent.width
						height: 28

						Text {
							textFormat: Text.PlainText
							anchors { left: parent.left; right: newGroupZone.left; rightMargin: 8; verticalCenter: parent.verticalCenter }
							text: qsTr("STASHED GROUPS")
							color: Theme.textMuted
							font { pixelSize: Theme.smallFontSize; bold: true; letterSpacing: 0.6 }
							elide: Text.ElideRight
						}

						Rectangle {
							id: newGroupZone
							objectName: "newStashGroupButton"
							anchors { right: parent.right; verticalCenter: parent.verticalCenter }
							width: newGroupRow.implicitWidth + 18
							height: 28
							radius: 14
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
						topPadding: 24
						bottomPadding: 24
						horizontalAlignment: Text.AlignHCenter
						text: Workspace.stash.loading ? qsTr("Loading…") : qsTr("No stashed groups yet")
						color: Theme.textMuted
						font.pixelSize: Theme.smallFontSize
					}

					Repeater {
						id: groupsRepeater
						model: Workspace.stash.groups

						delegate: Rectangle {
							id: groupCard
							required property var modelData
							required property int index

							readonly property string groupId: modelData.id ?? ""
							readonly property string groupName: modelData.name ?? ""
							readonly property bool groupLocked: modelData.locked === true
							readonly property var members: modelData.members ?? []
							readonly property bool editing: dialog.editingGroupId === groupId
							readonly property bool isDropTarget: dialog.dropTargetKind === "group" && dialog.dropGroupId === groupId
							readonly property bool beingDragged: dialog.dragKind === "group" && dialog.dragGroupId === groupId

							function memberHitAt(sceneX, sceneY) {
								for (let j = 0; j < memberRepeater.count; ++j) {
									const rowItem = memberRepeater.itemAt(j)
									if (rowItem && dialog.pointInItem(rowItem, sceneX, sceneY))
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

							width: groupsCol.width
							height: groupCol.implicitHeight + 16
							radius: Theme.radius
							color: Theme.surface
							border.color: (isDropTarget || beingDragged) ? Theme.accentText : Theme.border
							opacity: beingDragged ? 0.55 : 1
							Behavior on height { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
							Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }
							Behavior on border.color { ColorAnimation { duration: Motion.quick; easing.type: Motion.ease } }

							Column {
								id: groupCol
								x: 10
								y: 8
								width: parent.width - 20
								spacing: 6

								Item {
									id: groupHeader
									width: parent.width
									height: 28

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
											if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= 5) {
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
										height: 26
										radius: Theme.radiusSmall
										color: Theme.background
										border.color: Theme.accentText

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
									topPadding: 6
									bottomPadding: 6
									horizontalAlignment: Text.AlignHCenter
									text: qsTr("Drop sessions here")
									color: Theme.textMuted
									font.pixelSize: Theme.smallFontSize
								}

								Repeater {
									id: memberRepeater
									model: groupCard.members

									delegate: Rectangle {
										id: memberRow
										required property var modelData
										required property int index

										readonly property string sessionId: modelData.sessionId ?? ""
										readonly property string displayTitle: modelData.displayTitle ?? sessionId
										readonly property string dirName: modelData.dirName ?? ""
										readonly property bool live: modelData.live === true
										readonly property string rootId: modelData.rootId ?? ""
										readonly property color tint: dialog.memberTint(rootId, sessionId)
										readonly property bool isDropTarget: dialog.dropTargetKind === "member"
											&& dialog.dropGroupId === groupCard.groupId
											&& dialog.dropMemberSessionId === sessionId
										readonly property bool beingDragged: dialog.dragKind === "member"
											&& dialog.dragGroupId === groupCard.groupId
											&& dialog.dragSessionId === sessionId

										width: groupCol.width
										height: 46
										radius: Theme.radiusSmall
										color: live ? Theme.userBubble : Theme.background
										border.color: isDropTarget ? Theme.accentText : (live ? Theme.userBubbleBorder : Theme.border)
										opacity: beingDragged ? 0.45 : 1

										Behavior on opacity { NumberAnimation { duration: Motion.standard; easing.type: Motion.ease } }

										Rectangle {
											visible: memberRow.isDropTarget
											anchors { top: parent.top; left: parent.left; right: parent.right }
											height: 2
											color: Theme.accentText
										}

										Rectangle {
											x: 0
											y: 6
											width: 3
											height: parent.height - 12
											radius: 2
											color: memberRow.tint
										}

										Rectangle {
											x: 10
											anchors.verticalCenter: parent.verticalCenter
											width: 7
											height: 7
											radius: 3.5
											color: memberRow.tint
										}

										Column {
											anchors { left: parent.left; leftMargin: 24; right: memberActions.left; rightMargin: 6; verticalCenter: parent.verticalCenter }
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
												text: memberRow.live ? memberRow.dirName + " · " + qsTr("Live") : memberRow.dirName
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
												if (!started && Math.hypot(mouse.x - pressX, mouse.y - pressY) >= 5) {
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
			width: Math.min(220, dragProxyText.implicitWidth + 24)
			height: 30
			radius: 15
			color: Theme.userBubble
			border.color: dialog.dropTargetKind === "outside" ? Theme.danger : Theme.accentText

			Text {
				id: dragProxyText
				textFormat: Text.PlainText
				anchors { left: parent.left; right: parent.right; leftMargin: 12; rightMargin: 12; verticalCenter: parent.verticalCenter }
				text: dialog.dropTargetKind === "outside"
					? qsTr("Drop to remove: %1").arg(dialog.dragTitle)
					: dialog.dragTitle
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
