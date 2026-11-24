#include "workspace/session_directory.h"

#include <QLoggingCategory>
#include <QPointer>

#include <algorithm>
#include <chrono>

using namespace std::chrono_literals;

namespace refgui::workspace
{

namespace
{

Q_LOGGING_CATEGORY(lcDirectory, "refgui.workspace.directory")

// Session changes come in bursts (a turn ends, the title changes); fetch the history once.
constexpr auto HistoryDelay = 1000ms;
constexpr auto HistorySearchDelay = 150ms;

QString creatorWireValue(SessionListModel::CreatorFilter filter, const QString& creatorId)
{
	switch(filter)
	{
	case SessionListModel::CreatorFilter::All:
		return {};
	case SessionListModel::CreatorFilter::UserCreated:
		return QStringLiteral("user");
	case SessionListModel::CreatorFilter::ByCreator:
		return creatorId;
	}
	return {};
}

} // namespace

SessionDirectory::SessionDirectory(Connection& connection, QObject* parent)
	: QObject(parent)
	, m_backend(connection.backend())
{
	m_historyTimer.setSingleShot(true);
	m_historyTimer.setInterval(HistoryDelay);
	connect(&m_historyTimer, &QTimer::timeout, this, [this] {
		loadHistory();
		if(hasSidebarFilter())
		{
			loadUnfilteredHistory();
		}
	});
	m_historySearchTimer.setSingleShot(true);
	m_historySearchTimer.setInterval(HistorySearchDelay);
	connect(&m_historySearchTimer, &QTimer::timeout, this, &SessionDirectory::loadHistory);
	// Through the Connection, like the window's other handlers, so they run in the order they were connected.
	connect(&connection, &Connection::connected, this, [this] {
		loadLive();
		loadHistory();
		if(hasSidebarFilter())
		{
			loadUnfilteredHistory();
		}
		if(m_watchAll)
		{
			resubscribe();
		}
	});
	connect(&m_backend.client(), &client::ServerClient::frameReceived, this, &SessionDirectory::onFrame);
}

bool SessionDirectory::hasSidebarFilter() const
{
	return !m_historyCwd.isEmpty() || !m_historyCreator.isEmpty() || !m_historyQuery.isEmpty();
}

void SessionDirectory::setHistoryFilter(const QString& cwd, SessionListModel::CreatorFilter creatorFilter, const QString& creatorId, bool refreshNow)
{
	const QString creator = creatorWireValue(creatorFilter, creatorId);
	m_history.setFilters(cwd, creatorFilter, creatorId);
	if(cwd == m_historyCwd && creator == m_historyCreator)
	{
		return;
	}
	m_historyCwd = cwd;
	m_historyCreator = creator;
	if(refreshNow)
	{
		m_historySearchTimer.stop();
		loadHistory();
	}
}

void SessionDirectory::setHistoryQuery(const QString& query)
{
	const QString trimmed = query.trimmed();
	m_history.setFilter(trimmed);
	if(trimmed == m_historyQuery)
	{
		return;
	}
	m_historyQuery = trimmed;
	m_historySearchTimer.start();
}

void SessionDirectory::refreshHistory(bool immediate)
{
	if(immediate)
	{
		m_historyTimer.stop();
		m_historySearchTimer.stop();
		loadHistory();
		if(hasSidebarFilter())
		{
			loadUnfilteredHistory();
		}
		return;
	}
	m_historyTimer.start();
}

QList<proto::SessionInfo> SessionDirectory::allSessions() const
{
	QList<proto::SessionInfo> all = m_liveSessions;
	for(proto::SessionInfo info : m_persisted)
	{
		if(!m_live.contains(info.id))
		{
			info.live = false;
			all.append(std::move(info));
		}
	}
	return all;
}


const proto::SessionInfo* SessionDirectory::findAny(const QString& sessionId) const
{
	if(const proto::SessionInfo* liveInfo = m_live.find(sessionId))
	{
		return liveInfo;
	}
	const auto it = m_knownSessions.constFind(sessionId);
	return it == m_knownSessions.constEnd() ? nullptr : &*it;
}

void SessionDirectory::update(const proto::SessionInfo& info)
{
	QList<proto::SessionInfo> sessions = m_liveSessions;
	const auto same = [&info](const proto::SessionInfo& other) { return other.id == info.id; };
	const auto it = std::ranges::find_if(sessions, same);
	if(it == sessions.end())
	{
		sessions.append(info);
	}
	else
	{
		*it = info;
	}
	setLive(std::move(sessions));
}

void SessionDirectory::setLiveOrder(const QStringList& ids)
{
	m_live.setLiveOrder(ids);
	m_choices.setLiveOrder(ids);
}

void SessionDirectory::forgetPersisted(const QString& sessionId)
{
	m_persisted.removeIf([&sessionId](const proto::SessionInfo& info) { return info.id == sessionId; });
	m_sidebarPersisted.removeIf([&sessionId](const proto::SessionInfo& info) { return info.id == sessionId; });
	if(!m_live.contains(sessionId))
	{
		m_knownSessions.remove(sessionId);
	}
	applyHistory();
}

void SessionDirectory::reset()
{
	m_historyTimer.stop();
	m_historySearchTimer.stop();
	++m_historyRequest;
	++m_unfilteredHistoryRequest;
	m_liveSessions.clear();
	m_persisted.clear();
	m_sidebarPersisted.clear();
	m_knownSessions.clear();
	m_watchAll = false;
	m_live.setSessions({});
	m_history.setSessions({});
	m_choices.setSessions({});
	m_live.setKnownSessions({});
	m_history.setKnownSessions({});
	m_choices.setKnownSessions({});
	if(m_historyLoaded)
	{
		m_historyLoaded = false;
		emit historyLoadedChanged();
	}
	emit liveChanged();
	emit historyUpdated();
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::adopt(QCoro::Task<Result<proto::SessionInfo>> request)
{
	const QPointer<SessionDirectory> self(this); // the directory can be gone when the answer arrives
	const Result<proto::SessionInfo> info = co_await std::move(request);
	if(!info)
	{
		co_return std::unexpected(info.error());
	}
	if(self)
	{
		update(*info);
	}
	co_return Result<void>();
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::rename(QString sessionId, QString name)
{
	return adopt(m_backend.client().rename(std::move(sessionId), std::move(name)));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::setCwd(QString sessionId, QString path)
{
	return adopt(m_backend.client().setCwd(std::move(sessionId), std::move(path)));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::setModel(QString sessionId, proto::ModelRef model)
{
	return adopt(m_backend.client().setModel(std::move(sessionId), std::move(model)));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::setAgent(QString sessionId, QString agent)
{
	return adopt(m_backend.client().setAgent(std::move(sessionId), std::move(agent)));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::setVariant(QString sessionId, std::optional<QString> variant)
{
	return adopt(m_backend.client().setVariant(std::move(sessionId), std::move(variant)));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::compact(QString sessionId)
{
	return m_backend.client().compact(std::move(sessionId));
}

QCoro::Task<SessionDirectory::Result<QString>> SessionDirectory::exportTranscript(QString sessionId, QString path)
{
	return m_backend.client().exportTranscript(std::move(sessionId), std::move(path));
}

QCoro::Task<SessionDirectory::Result<QString>> SessionDirectory::peerCommand(QString sessionId, QString command, QString args)
{
	return m_backend.client().peerCommand(std::move(sessionId), std::move(command), std::move(args));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::close(QString sessionId)
{
	return m_backend.client().closeSession(std::move(sessionId));
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::remove(QString sessionId)
{
	return m_backend.client().deleteSession(std::move(sessionId));
}

QCoro::Task<SessionDirectory::Result<proto::SessionInfo>> SessionDirectory::create(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model)
{
	const QPointer<SessionDirectory> self(this); // the directory can be gone when the answer arrives
	const Result<proto::SessionInfo> created = co_await m_backend.client().createSession(std::move(cwd), std::move(agent), std::move(model));
	if(created && self)
	{
		update(*created);
	}
	co_return created;
}

QCoro::Task<SessionDirectory::Result<std::optional<proto::SessionInfo>>> SessionDirectory::newestIn(QString cwd)
{
	constexpr int NewestOnly = 1;
	const auto history = co_await m_backend.client().listHistory(std::move(cwd), NewestOnly);
	if(!history)
	{
		co_return std::unexpected(history.error());
	}
	co_return history->isEmpty() ? std::nullopt : std::optional<proto::SessionInfo>(history->first());
}

QCoro::Task<SessionDirectory::Result<proto::DirListing>> SessionDirectory::listDirs(QString path, std::optional<QString> cwd, bool includeFiles)
{
	return m_backend.client().listDirs(std::move(path), std::move(cwd), includeFiles);
}

QCoro::Task<SessionDirectory::Result<void>> SessionDirectory::setWatchAll(bool enabled)
{
	m_watchAll = enabled;
	const auto watching = co_await m_backend.client().watchAll(enabled);
	if(!watching)
	{
		co_return std::unexpected(watching.error());
	}
	co_return Result<void>();
}

QCoro::Task<> SessionDirectory::resubscribe()
{
	const auto watching = co_await m_backend.client().watchAll(true);
	if(!watching && !client::isConnectionLost(watching.error()))
	{
		qCWarning(lcDirectory).noquote() << "watching all sessions again failed:" << watching.error();
	}
}

QCoro::Task<> SessionDirectory::loadLive()
{
	const QPointer<SessionDirectory> self(this);
	const auto sessions = co_await m_backend.client().listSessions();
	if(!self)
	{
		co_return;
	}
	if(!sessions)
	{
		emit failed(sessions.error());
		co_return;
	}
	setLive(*sessions);
}

QCoro::Task<> SessionDirectory::loadHistory()
{
	if(m_backend.state() != client::Backend::State::Connected)
	{
		co_return;
	}
	const QPointer<SessionDirectory> self(this);
	const int request = ++m_historyRequest;
	const bool filtered = hasSidebarFilter();
	const auto sessions = co_await m_backend.client().listHistory(m_historyCwd, HistoryLimit, m_historyCreator, m_historyQuery);
	if(!self || request != m_historyRequest)
	{
		co_return;
	}
	if(!sessions)
	{
		emit failed(sessions.error());
		co_return;
	}
	m_sidebarPersisted = *sessions;
	if(!filtered)
	{
		++m_unfilteredHistoryRequest;
		m_persisted = *sessions;
	}
	for(proto::SessionInfo info : std::as_const(m_sidebarPersisted))
	{
		if(!m_live.contains(info.id))
		{
			info.live = false;
			m_knownSessions.insert(info.id, info);
		}
	}
	applyHistory();
	if(!m_historyLoaded)
	{
		m_historyLoaded = true;
		emit historyLoadedChanged();
	}
}

QCoro::Task<> SessionDirectory::loadUnfilteredHistory()
{
	if(m_backend.state() != client::Backend::State::Connected)
	{
		co_return;
	}
	const QPointer<SessionDirectory> self(this);
	const int request = ++m_unfilteredHistoryRequest;
	const auto sessions = co_await m_backend.client().listHistory({}, HistoryLimit);
	if(!self || request != m_unfilteredHistoryRequest)
	{
		co_return;
	}
	if(!sessions)
	{
		emit failed(sessions.error());
		co_return;
	}
	m_persisted = *sessions;
	for(proto::SessionInfo info : std::as_const(m_persisted))
	{
		if(!m_live.contains(info.id))
		{
			info.live = false;
			m_knownSessions.insert(info.id, info);
		}
	}
	applyHistory();
	if(!m_historyLoaded)
	{
		m_historyLoaded = true;
		emit historyLoadedChanged();
	}
}

void SessionDirectory::setLive(QList<proto::SessionInfo> sessions)
{
	std::ranges::sort(sessions, [](const proto::SessionInfo& a, const proto::SessionInfo& b) {
		return a.createdAt != b.createdAt ? a.createdAt > b.createdAt : a.id < b.id;
	});
	for(proto::SessionInfo& info : sessions)
	{
		info.live = true;
		m_knownSessions.insert(info.id, info);
	}
	m_liveSessions = sessions;
	applyHistory();
	m_live.setSessions(sessions);
	emit liveChanged();
}

void SessionDirectory::applyHistory()
{
	const auto nonLive = [this](const QList<proto::SessionInfo>& source) {
		QList<proto::SessionInfo> shown;
		for(proto::SessionInfo info : source)
		{
			if(!m_live.contains(info.id))
			{
				info.live = false;
				shown.append(std::move(info));
			}
		}
		return shown;
	};
	const QList<proto::SessionInfo> shownSidebar = nonLive(m_sidebarPersisted);
	const QList<proto::SessionInfo> shownAll = nonLive(m_persisted);
	const QList<proto::SessionInfo> known = m_knownSessions.values();
	m_history.setKnownSessions(known);
	m_choices.setKnownSessions(known);
	m_history.setSessions(shownSidebar);
	m_choices.setSessions(m_liveSessions + shownAll);
	emit historyUpdated();
}

void SessionDirectory::onFrame(const proto::ServerFrame& frame)
{
	const auto* sessions = std::get_if<proto::SessionsFrame>(&frame);
	if(sessions == nullptr)
	{
		return;
	}
	setLive(sessions->sessions);
	refreshHistory();
}

} // namespace refgui::workspace
