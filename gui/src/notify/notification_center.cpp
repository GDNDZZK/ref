#include "notify/notification_center.h"

#include "notify/notification_text.h"

#include <QLoggingCategory>

using namespace Qt::StringLiterals;

Q_LOGGING_CATEGORY(lcNotifyCenter, "refgui.notify.center")

namespace refgui::notify
{
namespace
{

using Kind = proto::NotifyFrame::Kind;

std::optional<Kind> kindOf(const QString& name)
{
	if(name == u"permission"_s)
	{
		return Kind::Permission;
	}
	if(name == u"turn_done"_s)
	{
		return Kind::TurnDone;
	}
	if(name == u"turn_error"_s)
	{
		return Kind::TurnError;
	}
	return std::nullopt;
}

} // namespace

NotificationCenter::NotificationCenter(client::Backend& backend, std::unique_ptr<Notifier> notifier, QObject* parent)
	: QObject(parent)
	, m_backend(backend)
	, m_notifier(std::move(notifier))
{
	connect(&m_backend.client(), &client::ServerClient::frameReceived, this, &NotificationCenter::onFrame);
	// A new connection starts with what Hello said; tell it what changed since. The kinds may
	// have changed while Hello was on its way, when nothing could be sent yet.
	connect(&m_backend, &client::Backend::connected, this,
	        [this]
	        {
		        if(m_backend.helloNotifyKinds() != m_kinds)
		        {
			        sendKinds();
		        }
		        sendFocus();
	        });
	connect(m_notifier.get(), &Notifier::availabilityChanged, this, &NotificationCenter::availabilityChanged);
	connect(m_notifier.get(), &Notifier::activated, this,
	        [this](const QString& sessionId)
	        {
		        raiseWindow();
		        emit sessionRequested(sessionId);
	        });
	connect(m_notifier.get(), &Notifier::showFailed, this,
	        [this](const Notification&, const QString& reason)
	        {
		        qCWarning(lcNotifyCenter) << "Cannot show the notification:" << reason;
		        getAttention();
	        });
}

void NotificationCenter::setKinds(const QStringList& kinds)
{
	QStringList names;
	QList<Kind> parsed;
	for(const QString& name : kinds)
	{
		if(const auto kind = kindOf(name); kind && !parsed.contains(*kind))
		{
			names.append(name);
			parsed.append(*kind);
		}
	}
	if(parsed == m_kinds)
	{
		return;
	}
	m_kindNames = names;
	m_kinds = parsed;
	m_backend.setNotifyKinds(m_kinds); // for the next connection's Hello
	if(m_backend.state() == client::Backend::State::Connected)
	{
		sendKinds();
	}
	emit kindsChanged();
}

void NotificationCenter::setFocus(bool focused, const QStringList& sessions)
{
	if(m_focusKnown && focused == m_focused && sessions == m_sessions)
	{
		return;
	}
	m_focusKnown = true;
	m_focused = focused;
	m_sessions = sessions;
	if(m_backend.state() == client::Backend::State::Connected)
	{
		sendFocus();
	}
}

QCoro::Task<> NotificationCenter::sendKinds()
{
	const auto result = co_await m_backend.client().setNotify(m_kinds);
	if(!result)
	{
		qCWarning(lcNotifyCenter) << "SetNotify failed:" << result.error();
	}
}

QCoro::Task<> NotificationCenter::sendFocus()
{
	if(!m_focusKnown)
	{
		co_return;
	}
	const auto result = co_await m_backend.client().setFocus(m_focused, m_sessions);
	if(!result)
	{
		qCWarning(lcNotifyCenter) << "SetFocus failed:" << result.error();
	}
}

void NotificationCenter::onFrame(const proto::ServerFrame& frame)
{
	const auto* notify = std::get_if<proto::NotifyFrame>(&frame);
	if(!notify)
	{
		return;
	}
	if(!m_notifier->isAvailable())
	{
		getAttention();
		return;
	}
	m_notifier->show(makeNotification(*notify));
}

void NotificationCenter::getAttention()
{
	if(m_window)
	{
		m_window->alert(0); // until the user looks at the window
	}
}

void NotificationCenter::raiseWindow()
{
	if(!m_window)
	{
		return;
	}
	if(m_window->windowStates() & Qt::WindowMinimized)
	{
		m_window->setWindowStates(m_window->windowStates() & ~Qt::WindowMinimized);
	}
	m_window->show();
	m_window->raise();
	m_window->requestActivate();
}

} // namespace refgui::notify
