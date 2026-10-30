// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"

#include <utility>

using refgui::client::Backend;
using namespace Qt::StringLiterals;

ChatSession::ChatSession(Backend::Source source, QString cwd, Start start, QObject* parent)
	: QObject(parent)
	, m_cwd(std::move(cwd))
	, m_start(std::move(start))
	, m_backend(std::move(source), m_cwd)
	, m_controller(m_backend)
{
	connect(&m_backend, &Backend::stateChanged, this, &ChatSession::connectionChanged);
	connect(&m_backend, &Backend::stateChanged, this, &ChatSession::statusTextChanged);
	connect(&m_backend, &Backend::connected, this, [this] {
		if(!m_controller.hasSession() && !m_opening)
		{
			open();
		}
	});
	connect(&m_controller, &refgui::session::SessionController::turnInProgressChanged, this, &ChatSession::busyChanged);
	connect(&m_controller, &refgui::session::SessionController::infoChanged, this, &ChatSession::titleChanged);
	connect(&m_controller, &refgui::session::SessionController::errorReported, this, &ChatSession::setFailure);
	connect(&m_controller, &refgui::session::SessionController::reattachFailed, this, &ChatSession::setFailure);
	connect(&m_controller, &refgui::session::SessionController::toastReceived, this, [this](const refgui::proto::ToastFrame& toast) {
		if(toast.level != refgui::proto::ToastFrame::Level::Info)
		{
			setFailure(toast.text);
		}
	});
	m_backend.start();
}

// The backend and the controller must go quiet before the members they call back into are destroyed.
ChatSession::~ChatSession()
{
	m_backend.shutdown();
}

QAbstractItemModel* ChatSession::blocks()
{
	return &m_controller.model();
}

bool ChatSession::isBusy() const
{
	return m_controller.isTurnInProgress();
}

QString ChatSession::connectionState() const
{
	switch(m_backend.state())
	{
	case Backend::State::Connecting:
		return u"connecting"_s;
	case Backend::State::StartingBackend:
		return u"starting"_s;
	case Backend::State::Connected:
		return u"connected"_s;
	case Backend::State::Disconnected:
		return u"disconnected"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QString ChatSession::statusText() const
{
	switch(m_backend.state())
	{
	case Backend::State::Connecting:
		return tr("Connecting to the ref backend…");
	case Backend::State::StartingBackend:
		return tr("Starting the ref backend…");
	case Backend::State::Disconnected:
		return tr("Disconnected: %1. Trying again…").arg(m_backend.reason());
	case Backend::State::Connected:
		return m_failure.isEmpty() ? m_notice : m_failure;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QString ChatSession::title() const
{
	return m_controller.info().title;
}

void ChatSession::send(const QString& text)
{
	Q_ASSERT(!isBusy() && !text.trimmed().isEmpty());
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

QCoro::Task<> ChatSession::open()
{
	m_opening = true;
	using Kind = Start::Kind;

	refgui::session::SessionController::Result<void> outcome;
	if(m_start.kind == Kind::Attach)
	{
		outcome = co_await m_controller.attach(m_start.sessionId);
	}
	else
	{
		bool needsNewSession = true;
		if(m_start.kind == Kind::Continue)
		{
			const auto last = co_await m_controller.continueLast(m_cwd);
			needsNewSession = last && !*last;
			if(needsNewSession)
			{
				setNotice(tr("No earlier session in this directory. Started a new one."));
			}
			else if(!last)
			{
				outcome = std::unexpected(last.error());
			}
		}
		if(needsNewSession)
		{
			outcome = co_await m_controller.create(m_cwd, m_start.agent, m_start.model);
		}
	}
	m_opening = false;

	if(!outcome)
	{
		setFailure(outcome.error());
		emit openFailed(outcome.error());
		co_return;
	}
	emit opened(m_controller.sessionId());
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

void ChatSession::setNotice(const QString& text)
{
	m_notice = text;
	emit statusTextChanged();
}
