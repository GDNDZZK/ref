#include "notify/dbus_notifier.h"

#include <QCoreApplication>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLoggingCategory>
#include <QVariantMap>

using namespace Qt::StringLiterals;

Q_LOGGING_CATEGORY(lcNotify, "refgui.notify")

namespace refgui::notify
{
namespace
{

const QString Service = u"org.freedesktop.Notifications"_s;
const QString Path = u"/org/freedesktop/Notifications"_s;
const QString Interface = u"org.freedesktop.Notifications"_s;

const QString AppName = u"ref"_s;
const QString AppIcon = u"ref-gui"_s;      // icon theme name, installed with the .desktop file
const QString DesktopEntry = u"ref-gui"_s; // lets the desktop group the notification under the app
const QString DefaultAction = u"default"_s; // what a click on the notification itself invokes
const QString BodyMarkupCapability = u"body-markup"_s;

// Urgency levels of the specification.
constexpr uchar UrgencyNormal = 1;
constexpr uchar UrgencyCritical = 2;
constexpr int ServerDefaultTimeout = -1;
constexpr uint NewNotification = 0; // replaces_id

QDBusMessage call(const QString& method)
{
	return QDBusMessage::createMethodCall(Service, Path, Interface, method);
}

// What a newer notification replaces: the one of the same session and kind, like the tag of a Web notification.
QString replaceKey(const Notification& notification)
{
	return u"%1:%2"_s.arg(notification.sessionId).arg(static_cast<int>(notification.kind));
}

} // namespace

DBusNotifier::DBusNotifier(QDBusConnection connection, QObject* parent)
	: Notifier(parent)
	, m_connection(std::move(connection))
	, m_watcher(Service, m_connection, QDBusServiceWatcher::WatchForOwnerChange)
{
	m_connection.connect(Service, Path, Interface, u"ActionInvoked"_s, this, SLOT(onActionInvoked(uint,QString)));
	m_connection.connect(Service, Path, Interface, u"NotificationClosed"_s, this, SLOT(onNotificationClosed(uint,uint)));
	connect(&m_watcher, &QDBusServiceWatcher::serviceOwnerChanged, this, &DBusNotifier::onServiceOwnerChanged);
	m_reason = tr("Looking for the desktop notification service…");
	probe();
}

// Asks the service who it is: an answer means notifications can be shown. Then it asks what the
// service can do, so the first notification is already worded for it.
void DBusNotifier::probe()
{
	auto* watcher = new QDBusPendingCallWatcher(m_connection.asyncCall(call(u"GetServerInformation"_s)), this);
	connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher* finished) {
		finished->deleteLater();
		if(finished->isError())
		{
			qCInfo(lcNotify) << "no notification service:" << finished->error().message();
			setAvailable(false, tr("No desktop notification service is running."));
			return;
		}
		probeCapabilities();
	});
}

void DBusNotifier::probeCapabilities()
{
	auto* watcher = new QDBusPendingCallWatcher(m_connection.asyncCall(call(u"GetCapabilities"_s)), this);
	connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher* finished) {
		finished->deleteLater();
		const QDBusPendingReply<QStringList> reply = *finished;
		// Without an answer the body stays plain text, which every service shows as it is.
		m_bodyMarkup = !reply.isError() && reply.value().contains(BodyMarkupCapability);
		setAvailable(true, {});
	});
}

void DBusNotifier::setAvailable(bool available, const QString& reason)
{
	if(m_available == available && m_reason == reason)
	{
		return;
	}
	m_available = available;
	m_reason = reason;
	emit availabilityChanged();
}

void DBusNotifier::onServiceOwnerChanged(const QString&, const QString&, const QString& newOwner)
{
	if(newOwner.isEmpty())
	{
		m_shown.clear();
		m_idOf.clear();
		setAvailable(false, tr("No desktop notification service is running."));
		return;
	}
	probe();
}

void DBusNotifier::show(const Notification& notification)
{
	if(!m_available)
	{
		return;
	}

	const bool critical = notification.kind != proto::NotifyFrame::Kind::TurnDone;
	const QVariantMap hints{
		{u"desktop-entry"_s, DesktopEntry},
		{u"urgency"_s, critical ? UrgencyCritical : UrgencyNormal},
	};
	const QString key = replaceKey(notification);
	// A service with markup reads the body as markup; the body is text from the conversation.
	const QString body = m_bodyMarkup ? notification.body.toHtmlEscaped() : notification.body;
	QDBusMessage message = call(u"Notify"_s);
	message << AppName << m_idOf.value(key, NewNotification) << AppIcon << notification.title << body
	        << QStringList{DefaultAction, tr("Open")} << hints << ServerDefaultTimeout;

	auto* watcher = new QDBusPendingCallWatcher(m_connection.asyncCall(message), this);
	connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, notification, key](QDBusPendingCallWatcher* finished) {
		finished->deleteLater();
		const QDBusPendingReply<uint> reply = *finished;
		if(reply.isError())
		{
			emit showFailed(notification, reply.error().message());
			return;
		}
		const uint id = reply.value();
		forget(m_idOf.value(key));
		m_shown.insert(id, {notification.sessionId, key});
		m_idOf.insert(key, id);
	});
}

void DBusNotifier::onActionInvoked(uint id, const QString& action)
{
	const auto shown = m_shown.constFind(id);
	if(action != DefaultAction || shown == m_shown.constEnd())
	{
		return;
	}
	emit activated(shown->sessionId);
}

void DBusNotifier::onNotificationClosed(uint id, uint)
{
	forget(id);
}

void DBusNotifier::forget(uint id)
{
	const Shown shown = m_shown.take(id);
	if(!shown.key.isEmpty() && m_idOf.value(shown.key) == id)
	{
		m_idOf.remove(shown.key);
	}
}

} // namespace refgui::notify
