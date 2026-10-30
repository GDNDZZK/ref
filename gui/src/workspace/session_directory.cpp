#include "workspace/session_directory.h"

#include <algorithm>
#include <chrono>

using namespace std::chrono_literals;

namespace refgui::workspace
{

namespace
{

// Session changes come in bursts (a turn ends, the title changes); fetch the history once.
constexpr auto HistoryDelay = 1000ms;

} // namespace

SessionDirectory::SessionDirectory(client::Backend& backend, QObject* parent)
	: QObject(parent)
	, m_backend(backend)
{
	m_historyTimer.setSingleShot(true);
	m_historyTimer.setInterval(HistoryDelay);
	connect(&m_historyTimer, &QTimer::timeout, this, &SessionDirectory::loadHistory);
	connect(&m_backend, &client::Backend::connected, this, [this] {
		loadLive();
		loadHistory();
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
	m_liveSessions = sessions;
	m_live.setSessions(sessions);
	applyHistory();
	emit liveChanged();
}

void SessionDirectory::applyHistory()
{
	QList<proto::SessionInfo> shown;
	for(const proto::SessionInfo& info : std::as_const(m_persisted))
	{
		if(!m_live.contains(info.id))
		{
			shown.append(info);
		}
	}
	m_history.setSessions(shown);
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
