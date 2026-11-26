// Split view: tiles of the live sessions nobody is attached to, as in the TUI.
#include "app/workspace.h"

#include "workspace/toast_text.h"

#include <algorithm>

using refgui::client::Result;
using namespace Qt::StringLiterals;
namespace
{
bool matchesSplitFilter(const refgui::proto::SessionInfo& info, const QString& filter)
{
	if(filter == u"user"_s)
	{
		return !info.parent || info.parent->isEmpty();
	}
	if(filter != u"all"_s && !filter.isEmpty())
	{
		return info.parent && *info.parent == filter;
	}
	return true;
}
} // namespace

void Workspace::toggleSplit()
{
	if(m_split)
	{
		leaveSplit();
		return;
	}
	enterSplit();
}

// The current session stays attached in the foreground (hidden while split view is on);
// WatchAll brings events for the live sessions.
QCoro::Task<> Workspace::enterSplit()
{
	m_lastSplitRetained.clear();
	m_tileAttachQueue.clear();
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
	m_stage.setRetainedIds({});
	m_tiles.setTiles({});
	m_directory.live().setTileIds({});
	m_tileAttachQueue.clear();
	m_lastSplitRetained.clear();
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
	for(ChatSession* other : std::as_const(m_chats))
	{
		if(other != nullptr && other != m_current && other->isAttached() && !m_leaving.contains(other->sessionId()))
		{
			detachUnwaited(other);
		}
	}
}

// Live, free background sessions excluding the current and leaving ones, oldest first.
QStringList Workspace::collectTileIds(bool applyFilter) const
{
	QList<const refgui::proto::SessionInfo*> infos;
	const QString currentSessionId = currentId();
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.id == currentSessionId || m_leaving.contains(info.id))
		{
			continue;
		}
		if(applyFilter && !matchesSplitFilter(info, m_splitFilter))
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
	ids.reserve(infos.size());
	for(const auto* info : infos)
	{
		ids.append(info->id);
	}
	return ids;
}

QStringList Workspace::backgroundTileIds() const
{
	return collectTileIds(false);
}

QStringList Workspace::wantedTileIds() const
{
	return collectTileIds(true);
}

QString Workspace::splitFilterLabel() const
{
	if(m_splitFilter.isEmpty() || m_splitFilter == u"all"_s || m_splitFilter == u"user"_s)
	{
		return {};
	}
	return m_directory.sessionLabel(m_splitFilter);
}

QVariantList Workspace::splitCreators() const
{
	QVariantList result;
	QSet<QString> seen;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.id == currentId())
		{
			continue;
		}
		const bool free = info.attached == 0 || info.id == m_zoomed || m_unzooming.contains(info.id);
		if(!info.live || !free || !info.parent || info.parent->isEmpty())
		{
			continue;
		}
		const QString& parentId = *info.parent;
		if(seen.contains(parentId))
		{
			continue;
		}
		seen.insert(parentId);
		result.append(QVariantMap{{u"id"_s, parentId}, {u"name"_s, m_directory.sessionLabel(parentId)}});
	}
	return result;
}

void Workspace::setSplitFilter(const QString& filter)
{
	QString next = filter.trimmed();
	if(next.isEmpty())
	{
		next = u"all"_s;
	}
	if(next == m_splitFilter)
	{
		return;
	}
	m_splitFilter = next;
	emit splitFilterChanged();
	if(m_split)
	{
		syncTiles();
	}
}

// Makes the tiles what they should be: new ones are attached as watchers, ones that left are let go.
void Workspace::syncTiles()
{
	if(!m_split)
	{
		return;
	}
	const QStringList wanted = wantedTileIds();
	if(!m_zoomed.isEmpty() && !wanted.contains(m_zoomed))
	{
		if(m_leaving.contains(m_zoomed))
		{
			m_zoomed.clear();
			emit zoomedChanged();
			updateCommandContext();
		}
		else
		{
			unzoom();
		}
	}
	const QStringList allBackground = backgroundTileIds();
	const QSet<QString> retained(allBackground.begin(), allBackground.end());
	if(retained != m_lastSplitRetained)
	{
		m_lastSplitRetained = retained;
		m_stage.setRetainedIds(retained);
	}

	const QList<ChatSession*> currentTiles = m_tiles.tiles();
	bool tilesChanged = currentTiles.size() != wanted.size();
	if(!tilesChanged)
	{
		for(qsizetype i = 0; i < wanted.size(); ++i)
		{
			if(currentTiles[i] == nullptr || currentTiles[i] != findSession(wanted[i]))
			{
				tilesChanged = true;
				break;
			}
		}
	}
	if(tilesChanged)
	{
		QList<ChatSession*> tiles;
		tiles.reserve(wanted.size());
		for(const QString& id : wanted)
		{
			tiles.append(&chat(id));
		}
		m_tiles.setTiles(tiles);
		m_directory.live().setTileIds(wanted);
		if(!m_focusedTile.isEmpty() && m_tiles.indexOf(m_focusedTile) < 0)
		{
			setFocusedTile({});
		}
		emit visibleSessionIdsChanged();
	}

	for(ChatSession* tile : m_tiles.tiles())
	{
		const QString id = tile->sessionId();
		if(!tile->isAttached() && !m_tileAttaching.contains(id) && !m_tileAttachQueue.contains(id))
		{
			m_tileAttachQueue.append(id);
		}
	}
	drainTileAttachQueue();

	for(ChatSession* other : std::as_const(m_chats))
	{
		const QString otherId = other->sessionId();
		const bool shouldDetachFromTiles = other->isAttached() && !retained.contains(otherId) && other != m_current && !m_leaving.contains(otherId);
		if(shouldDetachFromTiles)
		{
			detachUnwaited(other);
		}
	}
}

void Workspace::drainTileAttachQueue()
{
	while(m_split && m_tileAttaching.size() < MaxConcurrentTileAttaches && !m_tileAttachQueue.isEmpty())
	{
		const QString id = m_tileAttachQueue.takeFirst();
		if(m_tiles.indexOf(id) < 0 || m_leaving.contains(id) || m_tileAttaching.contains(id))
		{
			continue;
		}
		ChatSession* tile = findSession(id);
		if(tile == nullptr || tile->isAttached())
		{
			continue;
		}
		attachTile(tile);
	}
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
	drainTileAttachQueue();
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
