#include "session/session_controller.h"

#include "proto/types.h"

#include <QPointer>

#include <algorithm>
#include <utility>

namespace refgui::session
{
namespace
{

constexpr int NewestSessionOnly = 1;
// Messages asked for at a time when scrolling back; the server caps a page at 1000.
constexpr int OlderPageSize = 100;

// What a request that outlived its controller reports to whoever still waits for it.
QString destroyedError()
{
	return QStringLiteral("The session was closed.");
}

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
	const QPointer<SessionController> self(this); // the controller can be gone when the answer arrives
	const auto created = co_await m_backend.client().createSession(std::move(cwd), std::move(agent), std::move(model));
	if(!self)
	{
		co_return std::unexpected(destroyedError());
	}
	if(!created)
	{
		co_return std::unexpected(created.error());
	}
	co_return co_await attach(created->id);
}

QCoro::Task<SessionController::Result<void>> SessionController::attach(QString sessionId, bool foreground)
{
	const QPointer<SessionController> self(this); // the controller can be gone when the answer arrives
	const bool onlyModeChanges = m_attached && sessionId == m_sessionId;
	const auto snapshot = co_await m_backend.client().attach(sessionId, foreground);
	if(!self)
	{
		co_return std::unexpected(destroyedError());
	}
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

QCoro::Task<SessionController::Result<void>> SessionController::background()
{
	Q_ASSERT(m_attached);
	m_foreground = false;
	co_return co_await m_backend.client().detach(m_sessionId);
}

QCoro::Task<SessionController::Result<bool>> SessionController::continueLast(QString cwd)
{
	const QPointer<SessionController> self(this); // the controller can be gone when the answer arrives
	const auto history = co_await m_backend.client().listHistory(std::move(cwd), NewestSessionOnly);
	if(!self)
	{
		co_return std::unexpected(destroyedError());
	}
	if(!history)
	{
		co_return std::unexpected(history.error());
	}
	if(history->isEmpty())
	{
		co_return false;
	}
	const auto attached = co_await attach(history->first().id);
	if(!self)
	{
		co_return std::unexpected(destroyedError());
	}
	if(!attached)
	{
		co_return std::unexpected(attached.error());
	}
	co_return true;
}

QCoro::Task<SessionController::Result<void>> SessionController::loadOlder()
{
	Q_ASSERT(hasSession());
	const QString before = m_model.firstMessageId();
	if(!m_moreBefore || m_loadingOlder || before.isEmpty())
	{
		co_return Result<void>();
	}
	setLoadingOlder(true);
	const QPointer<SessionController> self(this); // the controller can be gone when the page arrives
	const QString sessionId = m_sessionId;
	const auto page = co_await m_backend.client().messages(sessionId, before, OlderPageSize);
	if(!self)
	{
		co_return std::unexpected(destroyedError());
	}
	// The conversation was replaced while the page was on its way: it does not fit any more.
	const bool stillFits = page && sessionId == m_sessionId && m_model.firstMessageId() == before;
	if(stillFits)
	{
		m_model.prependMessages(page->messages); // while loadingOlder still holds: the view takes them in quietly
		setMoreBefore(page->moreBefore);
	}
	setLoadingOlder(false);
	if(!page)
	{
		co_return std::unexpected(page.error());
	}
	co_return Result<void>();
}

QCoro::Task<SessionController::Result<proto::PromptResult>> SessionController::send(QString text, QList<proto::ImageData> images)
{
	Q_ASSERT(hasSession());
	m_model.setTailError({}); // a new prompt starts afresh
	co_return co_await m_backend.client().prompt(m_sessionId, std::move(text), std::move(images));
}

QCoro::Task<SessionController::Result<void>> SessionController::abort()
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().abort(m_sessionId);
}

QCoro::Task<SessionController::Result<proto::RemovedPrompt>> SessionController::undo(std::optional<QString> messageId)
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().undo(m_sessionId, std::move(messageId));
}

QCoro::Task<SessionController::Result<void>> SessionController::permissionReply(QString callId, bool allow, bool always)
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().permissionReply(m_sessionId, std::move(callId), allow, always);
}

QCoro::Task<SessionController::Result<void>> SessionController::questionReply(QString callId, std::optional<QList<QStringList>> answers)
{
	Q_ASSERT(hasSession());
	co_return co_await m_backend.client().questionReply(m_sessionId, std::move(callId), std::move(answers));
}

void SessionController::dropQuestion(const QString& callId)
{
	QList<proto::QuestionRequest> left = m_questions;
	left.removeIf([&](const proto::QuestionRequest& request) { return request.callId == callId; });
	if(left.size() != m_questions.size())
	{
		setQuestions(std::move(left));
	}
}

// The connection is new, so the backend knows nothing of our attachment.
QCoro::Task<> SessionController::reattach()
{
	const QPointer<SessionController> self(this); // the controller can be gone when the answer arrives
	const auto snapshot = co_await m_backend.client().attach(m_sessionId, m_foreground);
	if(!self)
	{
		co_return;
	}
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
	setQuestions(snapshot.pendingQuestions);
	setMoreBefore(snapshot.moreBefore);
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
	else if(const auto* question = std::get_if<proto::QuestionEvent>(&event))
	{
		const auto known = std::ranges::any_of(m_questions, [&](const proto::QuestionRequest& request) { return request.callId == question->request.callId; });
		if(!known)
		{
			QList<proto::QuestionRequest> questions = m_questions;
			questions.append(question->request);
			setQuestions(std::move(questions));
		}
	}
	else if(const auto* resolved = std::get_if<proto::QuestionResolvedEvent>(&event))
	{
		dropQuestion(resolved->callId);
	}
	else if(const auto* error = std::get_if<proto::ErrorEvent>(&event))
	{
		m_model.setTailError(error->text);
		emit errorReported(error->text);
	}
	else if(const auto* reset = std::get_if<proto::ResetEvent>(&event))
	{
		setMoreBefore(reset->moreBefore);
		m_feed.apply(event);
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

void SessionController::setQuestions(QList<proto::QuestionRequest> questions)
{
	m_questions = std::move(questions);
	emit questionsChanged();
}

void SessionController::setMoreBefore(bool moreBefore)
{
	if(moreBefore != m_moreBefore)
	{
		m_moreBefore = moreBefore;
		emit moreBeforeChanged();
	}
}

void SessionController::setLoadingOlder(bool loading)
{
	if(loading != m_loadingOlder)
	{
		m_loadingOlder = loading;
		emit loadingOlderChanged();
	}
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
