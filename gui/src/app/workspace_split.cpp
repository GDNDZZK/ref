// Split view: tiles of the live sessions nobody is attached to, as in the TUI.
#include "app/workspace.h"

#include "workspace/toast_text.h"

#include <QPointer>

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
	syncTiles();
	const Result<void> watching = co_await m_directory.setWatchAll(true);
	toastFailure(watching);
	if(!m_split)
	{
		co_return; // left again while the requests were out
	}
	syncTiles();
	emit visibleSessionIdsChanged();
	updateCommandContext();
}

// The current session (or the zoomed tile, or `openId`) comes back to the foreground; the others are let go.
QCoro::Task<> Workspace::leaveSplit(bool adoptZoomed, QString openId)
{
	ChatSession* handedOver = adoptZoomed && openId.isEmpty() && !m_zoomed.isEmpty() && !m_leaving.contains(m_zoomed)
	                              ? session(m_zoomed)
	                              : nullptr;
	if(handedOver != nullptr && handedOver != m_current)
	{
		++m_showSerial;
		m_current = handedOver;
		emit currentChanged();
	}

	m_split = false;
	const bool wasZoomed = !m_zoomed.isEmpty();
	m_zoomed.clear();
	setFocusedTile({});
	m_tiles.setTiles({});
	m_directory.live().setTileIds({});
	updateHighlight();
	if(wasZoomed)
	{
		emit zoomedChanged();
	}
	emit splitViewChanged();
	emit visibleSessionIdsChanged();
	updateCommandContext();

	if(!openId.isEmpty() && openId != currentId())
	{
		const Result<void> shown = co_await show(openId);
		toastFailure(shown);
	}
	if(m_split)
	{
		co_return;
	}
	if(m_current != nullptr && !m_current->isForeground())
	{
		const Result<void> resumed = co_await m_current->attach(true);
		toastFailure(resumed);
	}
	if(m_split)
	{
		co_return;
	}
	const Result<void> watching = co_await m_directory.setWatchAll(false);
	toastFailure(watching);
	if(m_split)
	{
		co_return;
	}
	// Sessions can be closed while a detach is out, which changes m_chats.
	QList<QPointer<ChatSession>> others;
	for(ChatSession* other : std::as_const(m_chats))
	{
		others.append(other);
	}
	for(const QPointer<ChatSession>& other : others)
	{
		if(m_split)
		{
			co_return;
		}
		if(other && other != m_current && other->isAttached() && !m_leaving.contains(other->sessionId()))
		{
			const QString otherId = other->sessionId();
			logDropped(co_await other->detach(), "detach"_L1, otherId);
		}
	}
}

// The sessions to show, oldest first: live background sessions nobody is attached to, excluding the current one.
QStringList Workspace::wantedTileIds() const
{
	QList<const refgui::proto::SessionInfo*> infos;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.id == currentId())
		{
			continue;
		}
		// While unzooming, the backend still counts the session as foreground until detach is answered.
		const bool free = info.attached == 0 || info.id == m_zoomed || m_unzooming.contains(info.id);
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
	QStringList ids;
	for(const QString& id : wanted)
	{
		if(m_leaving.contains(id))
		{
			continue;
		}
		tiles.append(&chat(id));
		ids.append(id);
	}
	m_tiles.setTiles(tiles);
	m_directory.live().setTileIds(ids);

	for(ChatSession* tile : std::as_const(tiles))
	{
		if(!tile->isAttached() && !m_tileAttaching.contains(tile->sessionId()))
		{
			attachTile(tile);
		}
	}
	for(ChatSession* other : std::as_const(m_chats))
	{
		const bool shouldDetachFromTiles = other->isAttached() && !tiles.contains(other) && other != m_current && !m_leaving.contains(other->sessionId());
		if(shouldDetachFromTiles)
		{
			detachUnwaited(other);
		}
	}
	if(!m_zoomed.isEmpty() && m_tiles.indexOf(m_zoomed) < 0)
	{
		m_zoomed.clear();
		emit zoomedChanged();
		updateCommandContext();
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
		toast(refgui::workspace::ToastLevel::Error, refgui::workspace::toastText(u"%1: %2"_s.arg(nameOf(id), refgui::workspace::oneLine(attached.error()))));
	}
}

void Workspace::setFocusedTile(const QString& sessionId)
{
	if(sessionId == m_focusedTile)
	{
		return;
	}
	m_focusedTile = sessionId;
	updateHighlight();
	emit focusedTileChanged();
	updateCommandContext();
}

void Workspace::focusTile(const QString& sessionId)
{
	if(!m_split)
	{
		return;
	}
	if(sessionId.isEmpty() || m_tiles.indexOf(sessionId) >= 0)
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
	if(ChatSession* previous = session(m_zoomed); previous != nullptr && previous->isAttached())
	{
		const QString previousId = previous->sessionId();
		m_unzooming.insert(previousId);
		logDropped(co_await previous->background(), "background"_L1, previousId);
		m_unzooming.remove(previousId);
		if(!m_split)
		{
			co_return;
		}
	}
	ChatSession* zoomed = session(sessionId);
	if(zoomed == nullptr || m_tiles.indexOf(sessionId) < 0)
	{
		if(!m_zoomed.isEmpty())
		{
			m_zoomed.clear();
			emit zoomedChanged();
			emit visibleSessionIdsChanged();
			updateCommandContext();
		}
		co_return; // closed meanwhile
	}
	m_zoomed = sessionId;
	setFocusedTile(sessionId);
	emit zoomedChanged();
	emit visibleSessionIdsChanged();
	updateCommandContext();
	const Result<void> attached = co_await zoomed->attach(true);
	toastFailure(attached);
	if(!attached && m_zoomed == sessionId && (session(sessionId) == nullptr || !session(sessionId)->isForeground()))
	{
		m_zoomed.clear();
		emit zoomedChanged();
		emit visibleSessionIdsChanged();
		updateCommandContext();
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
	updateCommandContext();
	ChatSession* tile = session(id);
	if(tile != nullptr && tile->isAttached())
	{
		// Until the answer, the backend counts the session as attached in the foreground, and an update of
		// its session list would take the tile away.
		m_unzooming.insert(id);
		logDropped(co_await tile->background(), "background"_L1, id);
		m_unzooming.remove(id);
	}
}
