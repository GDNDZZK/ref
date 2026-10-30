#include "session/session_controller.h"

#include "proto/types.h"

#include <utility>

namespace refgui::session
{
namespace
{

constexpr int NewestSessionOnly = 1;

} // namespace

SessionController::SessionController(client::Backend& backend, QObject* parent)
	: QObject(parent)
	, m_backend(backend)
	, m_model(this)
	, m_feed(m_model, this)
{
	connect(&backend.client(), &client::ServerClient::frameReceived, this, &SessionController::onFrame);
	connect(&backend, &client::Backend::connected, this, [this] {
		if(m_attached)
		{
			reattach();
		}
	});
	connect(&m_feed, &ConversationFeed::pendingChanged, this, &SessionController::turnInProgressChanged);
}

bool SessionController::isBusy() const
{
	return m_info.status.state != proto::SessionStatus::State::Idle;
}

bool SessionController::isTurnInProgress() const
{
	return isBusy() || m_feed.hasPending();
}

QCoro::Task<SessionController::Result<void>> SessionController::create(QString cwd,
                                                                       std::optional<QString> agent,
                                                                       std::optional<proto::ModelRef> model)
{
	const auto created = co_await m_backend.client().createSession(std::move(cwd), std::move(agent), std::move(model));
	if(!created)
	{
		co_return std::unexpected(created.error());
	}
	co_return co_await attach(created->id);
}

QCoro::Task<SessionController::Result<void>> SessionController::attach(QString sessionId, bool foreground)
{
	const bool onlyModeChanges = m_attached && sessionId == m_sessionId;
	const auto snapshot = co_await m_backend.client().attach(sessionId, foreground);
	if(!snapshot)
	{
		co_return std::unexpected(snapshot.error());
	}
	m_foreground = foreground;
	if(onlyModeChanges)
	{
		setInfo(snapshot->info);
		co_return Result<void>();
	}
	// Events that came before the snapshot are in it, or never come, so nothing is
	// buffered. The client resumes us before it handles the next frame.
	m_sessionId = std::move(sessionId);
	m_attached = true;
	adopt(*snapshot);
	co_return Result<void>();
}

QCoro::Task<SessionController::Result<void>> SessionController::detach()
{
	Q_ASSERT(m_attached);
	m_attached = false; // also when the request fails: a lost connection detaches anyway
	co_return co_await m_backend.client().detach(m_sessionId);
}

QCoro::Task<SessionController::Result<bool>> SessionController::continueLast(QString cwd)
{
	const auto history = co_await m_backend.client().listHistory(std::move(cwd), NewestSessionOnly);
	if(!history)
	{
		co_return std::unexpected(history.error());
	}
	if(history->isEmpty())
	{
		co_return false;
	}
	const auto attached = co_await attach(history->first().id);
	if(!attached)
	{
		co_return std::unexpected(attached.error());
	}
	co_return true;
}

QCoro::Task<SessionController::Result<proto::PromptResult>> SessionController::send(QString text)
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().prompt(m_sessionId, std::move(text));
}

QCoro::Task<SessionController::Result<void>> SessionController::abort()
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().abort(m_sessionId);
}

QCoro::Task<SessionController::Result<void>> SessionController::permissionReply(QString callId, bool allow, bool always)
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().permissionReply(m_sessionId, std::move(callId), allow, always);
}

// The connection is new, so the backend knows nothing of our attachment.
QCoro::Task<> SessionController::reattach()
{
	const auto snapshot = co_await m_backend.client().attach(m_sessionId, m_foreground);
	if(!snapshot)
	{
		emit reattachFailed(snapshot.error());
		co_return;
	}
	adopt(*snapshot);
}

void SessionController::adopt(const proto::SessionSnapshot& snapshot)
{
	m_feed.reset(snapshot.messages);
	setQueued(snapshot.queued);
	setInfo(snapshot.info);
}

void SessionController::onFrame(const proto::ServerFrame& frame)
{
	if(const auto* event = std::get_if<proto::EventFrame>(&frame))
	{
		// Detached, the model is behind; the next attach replaces it.
		if(m_attached && event->sessionId == m_sessionId)
		{
			onEvent(event->event);
		}
	}
}

void SessionController::onEvent(const proto::SessionEvent& event)
{
	if(const auto* info = std::get_if<proto::InfoEvent>(&event))
	{
		setInfo(info->info);
	}
	else if(const auto* queue = std::get_if<proto::QueueEvent>(&event))
	{
		setQueued(queue->messages);
	}
	else if(const auto* error = std::get_if<proto::ErrorEvent>(&event))
	{
		emit errorReported(error->text);
	}
	else
	{
		m_feed.apply(event); // ignores permission events: tool parts carry their state
	}
}

void SessionController::setQueued(QList<proto::Message> messages)
{
	m_queued = std::move(messages);
	emit queueChanged();
}

void SessionController::setInfo(const proto::SessionInfo& info)
{
	const bool wasBusy = isBusy();
	const bool wasInProgress = isTurnInProgress();
	m_info = info;
	emit infoChanged();
	if(isBusy() != wasBusy)
	{
		emit busyChanged();
	}
	if(isTurnInProgress() != wasInProgress)
	{
		emit turnInProgressChanged();
	}
}

} // namespace refgui::session
