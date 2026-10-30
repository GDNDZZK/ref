// Split view: tiles of the live sessions nobody is attached to, as in the TUI.
#include "app/workspace.h"

#include <algorithm>

using refgui::client::Result;
using namespace Qt::StringLiterals;

void Workspace::toggleSplit()
{
	if(m_split)
	{
		leaveSplit();
		return;
	}
	enterSplit();
}

// The current session becomes a background one like the others; its events then come
// through WatchAll, which also brings those of every other live session.
QCoro::Task<> Workspace::enterSplit()
{
	m_split = true;
	emit splitViewChanged();
	if(m_current != nullptr && m_current->isAttached())
	{
		const Result<void> demoted = co_await m_current->background();
		if(!demoted)
		{
			toast(u"error"_s, demoted.error());
		}
	}
	const auto watching = co_await m_backend.client().watchAll(true);
	if(!watching)
	{
		toast(u"error"_s, watching.error());
	}
	if(!m_split)
	{
		co_return; // left again while the requests were out
	}
	if(m_current != nullptr)
	{
		setFocusedTile(m_current->sessionId());
	}
	syncTiles();
	emit visibleSessionIdsChanged();
	updateCommandContext();
}

// The current session comes back to the foreground, the others are let go.
QCoro::Task<> Workspace::leaveSplit()
{
	m_split = false;
	m_zoomed.clear();
	setFocusedTile({});
	m_tiles.setTiles({});
	emit zoomedChanged();
	emit splitViewChanged();
	emit visibleSessionIdsChanged();
	updateCommandContext();

	if(m_current != nullptr && m_current->isAttached())
	{
		const Result<void> resumed = co_await m_current->attach(true);
		if(!resumed)
		{
			toast(u"error"_s, resumed.error());
		}
	}
	const auto watching = co_await m_backend.client().watchAll(false);
	if(!watching)
	{
		toast(u"error"_s, watching.error());
	}
	for(ChatSession* other : std::as_const(m_chats))
	{
		if(other != m_current && other->isAttached())
		{
			co_await other->detach();
		}
	}
}

// The sessions to show, oldest first: those no frontend is attached to, and the current one.
QStringList Workspace::wantedTileIds() const
{
	QList<const refgui::proto::SessionInfo*> infos;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		const bool free = info.attached == 0 || info.id == m_zoomed || info.id == currentId();
		if(info.live && free)
		{
			infos.append(&info);
		}
	}
	std::ranges::sort(infos, [](const auto* a, const auto* b) { return a->createdAt != b->createdAt ? a->createdAt < b->createdAt : a->id < b->id; });
	QStringList ids;
	for(const auto* info : infos)
	{
		ids.append(info->id);
	}
	if(!currentId().isEmpty() && !ids.contains(currentId()))
	{
		ids.append(currentId());
	}
	return ids;
}

// Makes the tiles what they should be: new ones are attached as watchers, ones that left are let go.
void Workspace::syncTiles()
{
	if(!m_split)
	{
		return;
	}
	const QStringList wanted = wantedTileIds();
	QList<ChatSession*> tiles;
	for(const QString& id : wanted)
	{
		if(m_leaving.contains(id))
		{
			continue;
		}
		tiles.append(&chat(id));
	}
	m_tiles.setTiles(tiles);

	for(ChatSession* tile : std::as_const(tiles))
	{
		if(!tile->isAttached() && !m_tileAttaching.contains(tile->sessionId()))
		{
			attachTile(tile);
		}
	}
	for(ChatSession* other : std::as_const(m_chats))
	{
		if(other->isAttached() && !tiles.contains(other) && other != m_current)
		{
			other->detach();
		}
	}
	if(!m_focusedTile.isEmpty() && m_tiles.indexOf(m_focusedTile) < 0)
	{
		setFocusedTile({});
	}
	emit visibleSessionIdsChanged();
}

QCoro::Task<> Workspace::attachTile(ChatSession* tile)
{
	const QString id = tile->sessionId();
	m_tileAttaching.insert(id);
	const Result<void> attached = co_await tile->attach(false);
	m_tileAttaching.remove(id);
	if(!attached)
	{
		toast(u"error"_s, u"%1: %2"_s.arg(nameOf(id), attached.error()));
	}
}

void Workspace::setFocusedTile(const QString& sessionId)
{
	if(sessionId == m_focusedTile)
	{
		return;
	}
	m_focusedTile = sessionId;
	emit focusedTileChanged();
}

void Workspace::focusTile(const QString& sessionId)
{
	if(m_split && m_tiles.indexOf(sessionId) >= 0)
	{
		setFocusedTile(sessionId);
	}
}

void Workspace::zoomTile(const QString& sessionId)
{
	if(!m_split || m_tiles.indexOf(sessionId) < 0 || sessionId == m_zoomed)
	{
		return;
	}
	zoomTask(sessionId);
}

QCoro::Task<> Workspace::zoomTask(QString sessionId)
{
	if(!m_zoomed.isEmpty())
	{
		ChatSession& previous = chat(m_zoomed);
		if(previous.isAttached())
		{
			co_await previous.background();
		}
	}
	m_zoomed = sessionId;
	setFocusedTile(sessionId);
	emit zoomedChanged();
	emit visibleSessionIdsChanged();
	const Result<void> attached = co_await chat(sessionId).attach(true);
	if(!attached)
	{
		toast(u"error"_s, attached.error());
	}
}

void Workspace::unzoom()
{
	if(m_zoomed.isEmpty())
	{
		return;
	}
	unzoomTask();
}

// A zoomed session was in the foreground; back in its tile it watches again.
QCoro::Task<> Workspace::unzoomTask()
{
	const QString id = std::exchange(m_zoomed, QString());
	emit zoomedChanged();
	emit visibleSessionIdsChanged();
	ChatSession& tile = chat(id);
	if(tile.isAttached())
	{
		co_await tile.background();
	}
}
