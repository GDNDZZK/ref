// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"

#include "composer/shared_history.h"

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
	connect(&m_controller, &refgui::session::SessionController::errorReported, this, &ChatSession::setFailure);
	connect(&m_controller, &refgui::session::SessionController::errorReported, this, &ChatSession::errorReported);
	connect(&m_controller, &refgui::session::SessionController::reattachFailed, this, &ChatSession::setFailure);
	connect(&m_composer, &refgui::composer::ComposerModel::failed, this, &ChatSession::setFailure);
	connect(&m_composer, &refgui::composer::ComposerModel::sent, this, [this] { setFailure({}); });
	connect(&m_composer, &refgui::composer::ComposerModel::interruptRequested, this, &ChatSession::stop);
}

QCoro::Task<refgui::client::Result<void>> ChatSession::attach(bool foreground)
{
	const auto result = co_await m_controller.attach(m_sessionId, foreground);
	emit readyChanged();
	co_return result;
}

QCoro::Task<refgui::client::Result<void>> ChatSession::detach()
{
	const auto result = co_await m_controller.detach();
	emit readyChanged();
	co_return result;
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

void ChatSession::permissionReply(const QString& callId, bool allow, bool always)
{
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
	const auto result = co_await m_controller.permissionReply(std::move(callId), allow, always);
	if(!result)
	{
		setFailure(result.error());
	}
}

void ChatSession::setFailure(const QString& text)
{
	m_failure = text;
	emit statusTextChanged();
}
