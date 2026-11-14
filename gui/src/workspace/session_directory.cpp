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

} // namespace

SessionDirectory::SessionDirectory(Connection& connection, QObject* parent)
	: QObject(parent)
	, m_backend(connection.backend())
{
	m_historyTimer.setSingleShot(true);
	m_historyTimer.setInterval(HistoryDelay);
	connect(&m_historyTimer, &QTimer::timeout, this, &SessionDirectory::loadHistory);
	// Through the Connection, like the window's other handlers, so they run in the order they were connected.
	connect(&connection, &Connection::connected, this, [this] {
		loadLive();
		loadHistory();
		if(m_watchAll)
		{
			resubscribe();
		}
	});
	connect(&m_backend.client(), &client::ServerClient::frameReceived, this, &SessionDirectory::onFrame);
}

void SessionDirectory::refreshHistory()
{
	m_historyTimer.start();
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

void SessionDirectory::forgetPersisted(const QString& sessionId)
{
	m_persisted.removeIf([&sessionId](const proto::SessionInfo& info) { return info.id == sessionId; });
	applyHistory();
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

QCoro::Task<SessionDirectory::Result<proto::DirListing>> SessionDirectory::listDirs(QString path)
{
	return m_backend.client().listDirs(std::move(path));
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
	const auto sessions = co_await m_backend.client().listSessions();
	if(!sessions)
	{
		emit failed(sessions.error());
		co_return;
	}
	setLive(*sessions);
}

QCoro::Task<> SessionDirectory::loadHistory()
{
	const int request = ++m_historyRequest;
	const auto sessions = co_await m_backend.client().listHistory({}, HistoryLimit);
	if(request != m_historyRequest)
	{
		co_return;
	}
	if(!sessions)
	{
		emit failed(sessions.error());
		co_return;
	}
	m_persisted = *sessions;
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
	}
	m_liveSessions = sessions;
	m_live.setSessions(sessions);
	applyHistory();
	emit liveChanged();
}

void SessionDirectory::applyHistory()
{
	QList<proto::SessionInfo> shown;
	for(proto::SessionInfo info : std::as_const(m_persisted))
	{
		if(!m_live.contains(info.id))
		{
			info.live = false;
			shown.append(info);
		}
	}
	m_history.setSessions(shown);
	m_choices.setSessions(m_liveSessions + shown);
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
