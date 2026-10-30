#pragma once

#include "notify/notifier.h"

#include <QDBusConnection>
#include <QDBusServiceWatcher>
#include <QHash>

namespace refgui::notify
{

// Talks to org.freedesktop.Notifications (the desktop notification service of GNOME, KDE,
// and most others). Available while a service answers on the bus. A click on the
// notification (its "default" action) activates the session. A newer notification of a
// session and kind replaces its older one instead of stacking. The body is text, also for
// services that read markup.
class DBusNotifier : public Notifier
{
	Q_OBJECT

public:
	// `connection` is the session bus, except in tests.
	explicit DBusNotifier(QDBusConnection connection, QObject* parent = nullptr);

	bool isAvailable() const override { return m_available; }
	QString unavailableReason() const override { return m_reason; }
	void show(const Notification& notification) override;

private slots:
	void onServiceOwnerChanged(const QString& service, const QString& oldOwner, const QString& newOwner);
	void onActionInvoked(uint id, const QString& action);
	void onNotificationClosed(uint id, uint reason);

private:
	void probe();
	void probeCapabilities();
	void setAvailable(bool available, const QString& reason);
	void forget(uint id);

	QDBusConnection m_connection;
	QDBusServiceWatcher m_watcher;
	bool m_available = false;
	bool m_bodyMarkup = false; // the service reads markup in the body
	QString m_reason;
	struct Shown
	{
		QString sessionId;
		QString key; // what replaces it
	};
	QHash<uint, Shown> m_shown;  // notification id -> what it shows
	QHash<QString, uint> m_idOf; // replace key -> its newest notification
};

} // namespace refgui::notify
