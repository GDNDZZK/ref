// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"

#include "composer/shared_history.h"
#include <QClipboard>
#include <QGuiApplication>

#include <utility>

using refgui::client::Backend;
using namespace Qt::StringLiterals;

ChatSession::ChatSession(Backend& backend, QString sessionId, QObject* parent)
	: QObject(parent)
	, m_backend(backend)
	, m_sessionId(std::move(sessionId))
	, m_controller(backend)
	, m_composer(backend, m_controller, refgui::composer::sharedPromptHistory())
{
	connect(&m_backend, &Backend::stateChanged, this, &ChatSession::readyChanged);
	connect(&m_controller, &refgui::session::SessionController::turnInProgressChanged, this, &ChatSession::busyChanged);
	connect(&m_controller, &refgui::session::SessionController::infoChanged, this, &ChatSession::titleChanged);
	connect(&m_controller, &refgui::session::SessionController::infoChanged, this, &ChatSession::infoChanged);
	connect(&m_controller, &refgui::session::SessionController::moreBeforeChanged, this, &ChatSession::moreBeforeChanged);
	connect(&m_controller, &refgui::session::SessionController::loadingOlderChanged, this, &ChatSession::loadingOlderChanged);
	connect(&m_controller, &refgui::session::SessionController::errorReported, this, &ChatSession::setFailure);
	connect(&m_controller, &refgui::session::SessionController::errorReported, this, &ChatSession::errorReported);
	connect(&m_controller, &refgui::session::SessionController::reattachFailed, this, &ChatSession::setFailure);
	connect(&m_composer, &refgui::composer::ComposerModel::failed, this, &ChatSession::setFailure);
	connect(&m_composer, &refgui::composer::ComposerModel::sent, this, [this] { setFailure({}); });
	connect(&m_composer, &refgui::composer::ComposerModel::interruptRequested, this, &ChatSession::stop);

	m_permissionGuard.setSingleShot(true);
	m_permissionGuard.setInterval(PermissionGuard);
	connect(&m_permissionGuard, &QTimer::timeout, this, [this] {
		m_permissionKeysActive = true;
		emit toolsChanged();
	});
	connect(&m_controller.model(), &refgui::session::ConversationModel::toolSummaryChanged, this, &ChatSession::trackOldestPermission);
}

QCoro::Task<refgui::client::Result<void>> ChatSession::attach(bool foreground)
{
	const auto result = co_await m_controller.attach(m_sessionId, foreground);
	emit readyChanged();
	co_return result;
}

QCoro::Task<refgui::client::Result<QByteArray>> ChatSession::fetchImage(QString imageId)
{
	const auto image = co_await m_backend.client().getImage(m_sessionId, std::move(imageId));
	if(!image)
	{
		co_return std::unexpected(image.error());
	}
	co_return QByteArray::fromBase64(image->data.toLatin1());
}

QCoro::Task<refgui::client::Result<void>> ChatSession::detach()
{
	const auto result = co_await m_controller.detach();
	emit readyChanged();
	co_return result;
}

QCoro::Task<refgui::client::Result<void>> ChatSession::background()
{
	co_return co_await m_controller.background();
}

QAbstractItemModel* ChatSession::blocks()
{
	return &m_controller.model();
}

bool ChatSession::isBusy() const
{
	return m_controller.isTurnInProgress();
}

bool ChatSession::isReady() const
{
	return m_backend.state() == Backend::State::Connected && m_controller.isAttached();
}

QString ChatSession::statusText() const
{
	return m_failure;
}

QString ChatSession::title() const
{
	return m_controller.info().title;
}

QString ChatSession::activity() const
{
	using State = refgui::proto::SessionStatus::State;
	switch(m_controller.info().status.state)
	{
	case State::Idle:
		return u"idle"_s;
	case State::Busy:
		return u"thinking"_s;
	case State::Running:
		return u"running"_s;
	case State::WaitingPermission:
		return u"waiting_permission"_s;
	case State::Retrying:
		return u"retrying"_s;
	case State::Compacting:
		return u"compacting"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

qint64 ChatSession::busySince() const
{
	return m_controller.info().busySince.value_or(0);
}

int ChatSession::retryAttempt() const
{
	return static_cast<int>(m_controller.info().status.attempt);
}

qint64 ChatSession::retryAt() const
{
	return m_controller.info().status.nextAt;
}

QString ChatSession::retryError() const
{
	return m_controller.info().status.error;
}

qint64 ChatSession::contextUsed() const
{
	return static_cast<qint64>(m_controller.info().context.used);
}

qint64 ChatSession::contextMax() const
{
	return static_cast<qint64>(m_controller.info().context.max);
}

qint64 ChatSession::contextRecommend() const
{
	return static_cast<qint64>(m_controller.info().context.recommend);
}

qint64 ChatSession::contextAuto() const
{
	return static_cast<qint64>(m_controller.info().context.autoCompact);
}

bool ChatSession::thinkingExpanded() const
{
	return m_controller.model().reasoningExpandedByDefault();
}

bool ChatSession::detailsExpanded() const
{
	return m_controller.model().toolsExpandedByDefault();
}

void ChatSession::toggleReasoning()
{
	m_controller.model().setDefaultExpansion(!thinkingExpanded(), detailsExpanded());
	emit expansionChanged();
}

void ChatSession::toggleDetails()
{
	m_controller.model().setDefaultExpansion(thinkingExpanded(), !detailsExpanded());
	emit expansionChanged();
}

void ChatSession::copyMessage(const QString& messageId)
{
	if(const auto text = m_controller.model().messageText(messageId))
	{
		QGuiApplication::clipboard()->setText(*text);
	}
}

void ChatSession::copyLastReply()
{
	copyMessage(m_controller.model().lastReplyId());
}

void ChatSession::loadOlder()
{
	fetchOlder();
}

QCoro::Task<> ChatSession::fetchOlder()
{
	const auto result = co_await m_controller.loadOlder();
	const QString error = result ? QString() : result.error();
	if(error != m_olderError)
	{
		m_olderError = error;
		emit olderErrorChanged();
	}
}

void ChatSession::send(const QString& text)
{
	Q_ASSERT(!isBusy() && !text.trimmed().isEmpty());
	if(!isReady())
	{
		setFailure(tr("Not connected to the ref backend."));
		return;
	}
	sendPrompt(text);
}

void ChatSession::stop()
{
	if(m_controller.isBusy())
	{
		sendAbort();
		return;
	}
	m_controller.showPendingNow(); // the server is done, only the display lags
}

bool ChatSession::lastToolDelayed() const
{
	return m_controller.model().toolSummary().lastToolDelayed;
}

int ChatSession::permissionCount() const
{
	return static_cast<int>(m_controller.model().toolSummary().awaitingPermission);
}

// A new oldest request restarts the guard; the keys are off until it runs out.
void ChatSession::trackOldestPermission()
{
	const QString& oldest = m_controller.model().toolSummary().oldestAwaitingCallId;
	if(oldest != m_guardedCallId)
	{
		m_guardedCallId = oldest;
		m_permissionKeysActive = false;
		m_permissionGuard.stop();
		if(!oldest.isEmpty())
		{
			m_permissionGuard.start();
		}
	}
	emit toolsChanged();
}

void ChatSession::answerOldestPermission(bool allow, bool always)
{
	if(!m_permissionKeysActive)
	{
		return;
	}
	permissionReply(m_guardedCallId, allow, always);
}

void ChatSession::undo(const QString& messageId)
{
	sendUndo(messageId);
}

void ChatSession::undoLast()
{
	sendUndo(std::nullopt);
}

QCoro::Task<> ChatSession::sendUndo(std::optional<QString> messageId)
{
	setFailure({});
	const auto result = co_await m_controller.undo(std::move(messageId));
	if(!result)
	{
		setFailure(result.error());
		co_return;
	}
	if(!result->text.isEmpty() || !result->images.isEmpty())
	{
		emit draftRestored(result->text, result->images);
	}
}

void ChatSession::cancelDelayedTool()
{
	stop();
}

void ChatSession::permissionReply(const QString& callId, bool allow, bool always)
{
	// The backend's answer takes a moment; a second reply to the same call would be refused.
	if(m_repliedCalls.contains(callId))
	{
		return;
	}
	m_repliedCalls.insert(callId);
	sendPermission(callId, allow, always);
}

QCoro::Task<> ChatSession::sendPrompt(QString text)
{
	setFailure({});
	const auto result = co_await m_controller.send(std::move(text));
	if(!result)
	{
		setFailure(result.error());
	}
}

QCoro::Task<> ChatSession::sendAbort()
{
	const auto result = co_await m_controller.abort();
	if(!result)
	{
		setFailure(result.error());
	}
}

QCoro::Task<> ChatSession::sendPermission(QString callId, bool allow, bool always)
{
	const auto result = co_await m_controller.permissionReply(callId, allow, always);
	if(!result)
	{
		m_repliedCalls.remove(callId); // it may be tried again
		setFailure(result.error());
	}
}

void ChatSession::setFailure(const QString& text)
{
	m_failure = text;
	emit statusTextChanged();
}
